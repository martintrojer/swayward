use super::*;

impl Swayward {
    fn ensure_lock_redraw(&mut self, output: &Output) {
        let output = output.downgrade();
        self.event_loop
            .insert_source(
                Timer::from_duration(Duration::from_millis(100)),
                move |_, _, state| {
                    if !matches!(state.swayward.lock_state, LockState::Locking(_)) {
                        return TimeoutAction::Drop;
                    }
                    let Some(output) = output.upgrade() else {
                        return TimeoutAction::Drop;
                    };
                    let Some(output_state) = state.swayward.output_state.get_mut(&output) else {
                        return TimeoutAction::Drop;
                    };
                    if output_state.lock_render_state == LockRenderState::Locked {
                        return TimeoutAction::Drop;
                    }

                    output_state.redraw_state =
                        mem::take(&mut output_state.redraw_state).queue_redraw();
                    TimeoutAction::ToDuration(Duration::from_millis(100))
                },
            )
            .unwrap();
    }

    #[cfg(test)]
    pub(crate) fn set_test_lock_deadline(&mut self, deadline: Duration) {
        self.lock_deadline = deadline;
    }

    pub fn is_locked(&self) -> bool {
        match self.lock_state {
            LockState::Unlocked | LockState::WaitingForSurfaces { .. } => false,
            LockState::Locking(_) | LockState::Locked(_) => true,
        }
    }

    pub fn lock(&mut self, confirmation: SessionLocker) {
        // Check if another client is in the process of locking.
        if matches!(
            self.lock_state,
            LockState::WaitingForSurfaces { .. } | LockState::Locking(_)
        ) {
            info!("refusing lock as another client is currently locking");
            return;
        }

        // Check if we're already locked with an active client.
        if let LockState::Locked(lock) = &self.lock_state {
            if lock.is_alive() {
                info!("refusing lock as already locked with an active client");
                return;
            }

            // If the client had died, continue with the new lock.
            info!("locking session (replacing existing dead lock)");

            // Since the session was already locked, we know that the outputs are blanked, and
            // can lock right away.
            let lock = confirmation.ext_session_lock().clone();
            confirmation.lock();
            self.lock_state = LockState::Locked(lock);

            return;
        }

        info!("locking session");

        if self.output_state.is_empty() {
            // There are no outputs, lock the session right away.
            self.screenshot_ui.close();
            self.cursor_manager
                .set_cursor_image(CursorImageStatus::default_named());

            let lock = confirmation.ext_session_lock().clone();
            confirmation.lock();
            self.lock_state = LockState::Locked(lock);
        } else {
            // There are outputs which we need to redraw before locking. But before we do that,
            // let's wait for the lock surfaces.
            //
            // Give them a second; swaylock can take its time to paint a big enough image.
            #[cfg(not(test))]
            let lock_deadline = Duration::from_millis(1000);
            #[cfg(test)]
            let lock_deadline = self.lock_deadline;
            let timer = Timer::from_duration(lock_deadline);
            let deadline_token = self
                .event_loop
                .insert_source(timer, |_, _, state| {
                    trace!("lock deadline expired, continuing");
                    state.swayward.continue_to_locking();
                    TimeoutAction::Drop
                })
                .unwrap();

            self.lock_state = LockState::WaitingForSurfaces {
                confirmation,
                deadline_token,
            };
        }
    }

    pub fn maybe_continue_to_locking(&mut self) {
        if !matches!(self.lock_state, LockState::WaitingForSurfaces { .. }) {
            // Not waiting.
            return;
        }

        // Check if there are any outputs whose lock surfaces had not had a commit yet.
        for state in self.output_state.values() {
            let Some(surface) = &state.lock_surface else {
                // Surface not created yet.
                return;
            };

            if !is_mapped(surface.wl_surface()) {
                return;
            }
        }

        // All good.
        trace!("lock surfaces are ready, continuing");
        self.continue_to_locking();
    }

    fn continue_to_locking(&mut self) {
        match mem::take(&mut self.lock_state) {
            LockState::WaitingForSurfaces {
                confirmation,
                deadline_token,
            } => {
                self.event_loop.remove(deadline_token);

                self.screenshot_ui.close();
                self.cursor_manager
                    .set_cursor_image(CursorImageStatus::default_named());
                self.cancel_mru();

                if self.output_state.is_empty() {
                    // There are no outputs, lock the session right away.
                    let lock = confirmation.ext_session_lock().clone();
                    confirmation.lock();
                    self.lock_state = LockState::Locked(lock);
                } else {
                    // There are outputs which we need to redraw before locking.
                    self.lock_state = LockState::Locking(confirmation);
                    self.queue_redraw_all();
                    for output in self.output_state.keys().cloned().collect::<Vec<_>>() {
                        self.ensure_lock_redraw(&output);
                    }
                }
            }
            other => {
                error!("continue_to_locking() called with wrong lock state: {other:?}",);
                self.lock_state = other;
            }
        }
    }

    pub fn unlock(&mut self) {
        info!("unlocking session");

        let prev = mem::take(&mut self.lock_state);
        if let LockState::WaitingForSurfaces { deadline_token, .. } = prev {
            self.event_loop.remove(deadline_token);
        }

        for output_state in self.output_state.values_mut() {
            output_state.lock_surface = None;
        }
        self.queue_redraw_all();
    }

    #[cfg(feature = "dbus")]
    pub(super) fn update_locked_hint(&mut self) {
        use std::sync::LazyLock;

        if !self.is_session_instance {
            return;
        }

        static XDG_SESSION_ID: LazyLock<Option<String>> = LazyLock::new(|| {
            let id = std::env::var("XDG_SESSION_ID").ok();
            if id.is_none() {
                warn!(
                    "env var 'XDG_SESSION_ID' is unset or invalid; logind LockedHint won't be set"
                );
            }
            id
        });

        let Some(session_id) = &*XDG_SESSION_ID else {
            return;
        };

        fn call(session_id: &str, locked: bool) -> anyhow::Result<()> {
            let conn = zbus::blocking::Connection::system()
                .context("error connecting to the system bus")?;

            let message = conn
                .call_method(
                    Some("org.freedesktop.login1"),
                    "/org/freedesktop/login1",
                    Some("org.freedesktop.login1.Manager"),
                    "GetSession",
                    &(session_id),
                )
                .context("failed to call GetSession")?;

            let message_body = message.body();
            let session_path: zbus::zvariant::ObjectPath = message_body
                .deserialize()
                .context("failed to deserialize GetSession reply")?;

            conn.call_method(
                Some("org.freedesktop.login1"),
                session_path,
                Some("org.freedesktop.login1.Session"),
                "SetLockedHint",
                &(locked),
            )
            .context("failed to call SetLockedHint")?;

            Ok(())
        }

        // Consider only the fully locked state here. When using the locked hint with sleep
        // inhibitor tools, we want to allow sleep only after the screens are fully cleared with
        // the lock screen, which corresponds to the Locked state.
        let locked = matches!(self.lock_state, LockState::Locked(_));

        if self.locked_hint.is_some_and(|h| h == locked) {
            return;
        }

        self.locked_hint = Some(locked);

        let res = thread::Builder::new()
            .name("Logind LockedHint Updater".to_owned())
            .spawn(move || {
                let _span = tracy_client::span!("LockedHint");

                if let Err(err) = call(session_id, locked) {
                    warn!("failed to set logind LockedHint: {err:?}");
                }
            });

        if let Err(err) = res {
            warn!("error spawning a thread to set logind LockedHint: {err:?}");
        }
    }

    pub fn new_lock_surface(&mut self, surface: LockSurface, output: &Output) {
        let lock = match &self.lock_state {
            LockState::Unlocked => {
                error!("tried to add a lock surface on an unlocked session");
                return;
            }
            LockState::WaitingForSurfaces { confirmation, .. } => confirmation.ext_session_lock(),
            LockState::Locking(confirmation) => confirmation.ext_session_lock(),
            LockState::Locked(lock) => lock,
        };

        if lock.client() != surface.wl_surface().client() {
            debug!("ignoring lock surface from an unrelated client");
            return;
        }

        if lock != surface.ext_session_lock() {
            debug!("ignoring lock surface from an unrelated lock instance");
            return;
        }

        let Some(output_state) = self.output_state.get_mut(output) else {
            error!("missing output state");
            return;
        };

        output_state.lock_surface = Some(surface);
    }
}
