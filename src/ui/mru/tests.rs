use proptest::prelude::*;
use proptest_derive::Arbitrary;

use super::*;
use crate::tests::fixture::Fixture;

fn create_thumbnail() -> Thumbnail {
    Thumbnail {
        id: MappedId::next(),
        timestamp: None,
        on_current_output: false,
        on_current_workspace: false,
        app_id: None,
        size: Size::new(100, 100),
        clock: Clock::with_time(Duration::ZERO),
        config: swayward_config::MruPreviews::default(),
        open_animation: None,
        move_animation: None,
        title_texture: Default::default(),
        background: RefCell::new(FocusRing::new(Default::default())),
        border: RefCell::new(FocusRing::new(Default::default())),
    }
}

fn bind(trigger: Keysym, action: Action) -> Bind {
    Bind {
        key: Key {
            trigger: Trigger::Keysym(trigger),
            modifiers: Modifiers::COMPOSITOR,
        },
        action,
        repeat: true,
        cooldown: None,
        allow_when_locked: false,
        allow_inhibiting: false,
        hotkey_overlay_title: None,
    }
}

#[test]
fn inherited_focus_binds_remain_mru_navigation_aliases() {
    let mut config = Config::default();
    config.binds.0 = vec![
        bind(Keysym::h, Action::FocusColumnLeft),
        bind(Keysym::l, Action::FocusColumnRight),
        bind(Keysym::Home, Action::FocusColumnFirst),
        bind(Keysym::End, Action::FocusColumnLast),
        bind(Keysym::j, Action::SwayCommand("focus down".into())),
    ];

    let binds = make_dynamic_opened_binds(&config);
    assert!(binds.iter().any(|bind| {
        bind.key.trigger == Trigger::Keysym(Keysym::h)
            && matches!(
                bind.action,
                Action::MruAdvance {
                    direction: MruDirection::Backward,
                    ..
                }
            )
    }));
    assert!(binds.iter().any(|bind| {
        bind.key.trigger == Trigger::Keysym(Keysym::l)
            && matches!(
                bind.action,
                Action::MruAdvance {
                    direction: MruDirection::Forward,
                    ..
                }
            )
    }));
    assert!(binds
        .iter()
        .any(|bind| bind.key.trigger == Trigger::Keysym(Keysym::Home)
            && bind.action == Action::MruFirst));
    assert!(binds
        .iter()
        .any(|bind| bind.key.trigger == Trigger::Keysym(Keysym::End)
            && bind.action == Action::MruLast));
    assert!(!binds
        .iter()
        .any(|bind| bind.key.trigger == Trigger::Keysym(Keysym::j)));
}

#[test]
fn mapped_navigation_aliases_follow_mru_order_in_a_nested_tree() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1200, 800));
    let client = fixture.add_client();
    let mut ids = Vec::new();
    for command in [None, Some("split v"), Some("split h")] {
        if let Some(command) = command {
            assert!(crate::command::execute(fixture.niri_state(), command)[0].success);
        }
        let window = fixture.client(client).create_window();
        let surface = window.surface.clone();
        window.commit();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
        ids.push(fixture.swayward().layout.focus().unwrap().id());
    }
    assert!(crate::command::execute(fixture.niri_state(), "layout tabbed")[0].success);

    let mut mru = WindowMru::new(fixture.swayward());
    assert_eq!(mru.current_id, Some(ids[2]));

    let mut config = Config::default();
    config.binds.0 = vec![
        bind(Keysym::h, Action::FocusColumnLeft),
        bind(Keysym::l, Action::FocusColumnRight),
    ];
    let binds = make_dynamic_opened_binds(&config);
    let direction = |key| {
        let action = &binds
            .iter()
            .find(|bind| bind.key.trigger == Trigger::Keysym(key))
            .unwrap()
            .action;
        let Action::MruAdvance { direction, .. } = action else {
            panic!("unexpected action: {action:?}")
        };
        *direction
    };

    for (key, expected) in [
        (Keysym::l, ids[1]),
        (Keysym::l, ids[0]),
        (Keysym::h, ids[1]),
    ] {
        match direction(key) {
            MruDirection::Forward => mru.forward(),
            MruDirection::Backward => mru.backward(),
        }
        assert_eq!(mru.current_id, Some(expected));
    }
}

#[test]
fn remove_last_window_out_of_two() {
    let ops = [Op::Backward, Op::Remove(1)];

    let thumbnails = vec![create_thumbnail(), create_thumbnail()];
    let current_id = thumbnails.first().map(|t| t.id);
    let mut mru = WindowMru {
        thumbnails,
        current_id,
        scope: MruScope::All,
        app_id_filter: None,
    };

    check_ops(&mut mru, &ops);
}

fn arbitrary_scope() -> impl Strategy<Value = MruScope> {
    prop_oneof![
        Just(MruScope::All),
        Just(MruScope::Output),
        Just(MruScope::Workspace),
    ]
}

fn arbitrary_filter() -> impl Strategy<Value = MruFilter> {
    prop_oneof![Just(MruFilter::All), Just(MruFilter::AppId)]
}

fn arbitrary_app_id() -> impl Strategy<Value = Option<String>> {
    prop_oneof![Just(None), Just(Some(1)), Just(Some(2))]
        .prop_map(|id| id.map(|id| format!("app-{id}")))
}

prop_compose! {
    fn arbitrary_thumbnail()(
        timestamp: Option<Duration>,
        on_current_output: bool,
        on_current_workspace: bool,
        app_id in arbitrary_app_id(),
    ) -> Thumbnail {
        let mut thumbnail = create_thumbnail();
        thumbnail.timestamp = timestamp;
        thumbnail.on_current_workspace = on_current_workspace;
        thumbnail.on_current_output = on_current_output;
        thumbnail.app_id = app_id;
        thumbnail
    }
}

prop_compose! {
    fn arbitrary_mru()(
        thumbnails in proptest::collection::vec(arbitrary_thumbnail(), 1..10),
    ) -> WindowMru {
        let current_id = thumbnails.first().map(|t| t.id);
        WindowMru {
            thumbnails,
            current_id,
            scope: MruScope::All,
            app_id_filter: None,
        }
    }
}

#[derive(Debug, Clone, Arbitrary)]
enum Op {
    Forward,
    Backward,
    First,
    Last,
    SetScope(#[proptest(strategy = "arbitrary_scope()")] MruScope),
    SetFilter(#[proptest(strategy = "arbitrary_filter()")] MruFilter),
    Remove(#[proptest(strategy = "1..10usize")] usize),
}

impl Op {
    fn apply(&self, mru: &mut WindowMru) {
        match self {
            Op::Forward => mru.forward(),
            Op::Backward => mru.backward(),
            Op::First => mru.first(),
            Op::Last => mru.last(),
            Op::SetScope(scope) => {
                mru.set_scope(*scope);
            }
            Op::SetFilter(filter) => {
                mru.set_filter(*filter);
            }
            Op::Remove(idx) => {
                if *idx < mru.thumbnails.len() {
                    mru.remove_by_idx(*idx);
                }
            }
        }
    }
}

fn check_ops(mru: &mut WindowMru, ops: &[Op]) {
    for op in ops {
        op.apply(mru);
        mru.verify_invariants();
    }
}

proptest! {
    #[test]
    fn random_operations_dont_panic(
        mut mru in arbitrary_mru(),
        ops: Vec<Op>,
    ) {
        check_ops(&mut mru, &ops);
    }
}
