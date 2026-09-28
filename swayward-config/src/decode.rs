use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use knuffel::errors::DecodeError;
use knuffel::Decode as _;
use miette::{miette, Context as _};

use crate::error::{ConfigIncludeError, ConfigParseResult};
use crate::recent_windows::RecentWindowsPart;
use crate::utils::{parse_arg_node, Flag, MergeWith as _};
use crate::*;

const RECURSION_LIMIT: u8 = 10;

// Newtypes for putting information into the knuffel context.
struct BasePath(PathBuf);
struct RootBase(PathBuf);
struct Recursion(u8);
#[derive(Default)]
struct Includes(Vec<PathBuf>);
#[derive(Default)]
struct IncludeErrors(Vec<knuffel::Error>);
// Used for recursive include detection.
//
// We don't *need* it because we have a recursion limit, but it makes for nicer error messages.
struct IncludeStack(HashSet<PathBuf>);
struct SawMruBinds(Rc<Cell<bool>>);

// Rather than listing all fields and deriving knuffel::Decode, we implement
// knuffel::DecodeChildren by hand, since we need custom logic for every field anyway: we want to
// merge the values into the config from the context as we go to support the positionality of
// includes. The reason we need this type at all is because knuffel's only entry point that allows
// setting default values on a context is `parse_with_context()` that needs a type to parse.
pub struct ConfigPart;

impl<S> knuffel::DecodeChildren<S> for ConfigPart
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_children(
        nodes: &[knuffel::ast::SpannedNode<S>],
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        let _span = tracy_client::span!("decode config file");

        let config = ctx.get::<Rc<RefCell<Config>>>().unwrap().clone();
        let includes = ctx.get::<Rc<RefCell<Includes>>>().unwrap().clone();
        let include_errors = ctx.get::<Rc<RefCell<IncludeErrors>>>().unwrap().clone();
        let recursion = ctx.get::<Recursion>().unwrap().0;
        let saw_mru_binds = ctx.get::<SawMruBinds>().unwrap().0.clone();

        let mut seen = HashSet::new();

        for node in nodes {
            let name = &**node.node_name;

            // Within one config file, splitting sections into multiple parts is not allowed to
            // reduce confusion. The exceptions here aren't multipart; they all add new values.
            if !matches!(
                name,
                "output"
                    | "spawn-at-startup"
                    | "spawn-sh-at-startup"
                    | "window-rule"
                    | "layer-rule"
                    | "workspace"
                    | "mode"
                    | "include"
            ) && !seen.insert(name)
            {
                ctx.emit_error(DecodeError::unexpected(
                    &node.node_name,
                    "node",
                    format!("duplicate node `{name}`, single node expected"),
                ));
                continue;
            }

            macro_rules! m_merge {
                ($field:ident) => {{
                    let part = knuffel::Decode::decode_node(node, ctx)?;
                    config.borrow_mut().$field.merge_with(&part);
                }};
            }

            macro_rules! m_push {
                ($field:ident) => {{
                    let part = knuffel::Decode::decode_node(node, ctx)?;
                    config.borrow_mut().$field.push(part);
                }};
            }

            match name {
                "input" => m_merge!(input),
                "cursor" => m_merge!(cursor),
                "clipboard" => m_merge!(clipboard),
                "hotkey-overlay" => m_merge!(hotkey_overlay),
                "config-notification" => m_merge!(config_notification),
                "animations" => m_merge!(animations),
                "blur" => m_merge!(blur),
                "gestures" => m_merge!(gestures),
                "overview" => m_merge!(overview),
                "xwayland-satellite" => m_merge!(xwayland_satellite),
                "switch-events" => m_merge!(switch_events),
                "debug" => m_merge!(debug),

                // Multipart sections.
                "output" => {
                    let part = Output::decode_node(node, ctx)?;
                    config.borrow_mut().outputs.0.push(part);
                }
                "spawn-at-startup" => m_push!(spawn_at_startup),
                "spawn-sh-at-startup" => m_push!(spawn_sh_at_startup),
                "window-rule" => m_push!(window_rules),
                "layer-rule" => m_push!(layer_rules),
                "workspace" => {
                    let workspace = Workspace::decode_node(node, ctx)?;
                    if workspace
                        .sway_output_assignment
                        .as_ref()
                        .is_some_and(Vec::is_empty)
                    {
                        ctx.emit_error(DecodeError::unexpected(
                            node,
                            "workspace",
                            "sway-output-assignment requires at least one output",
                        ));
                    }
                    if workspace.sway_output_assignment.is_some()
                        && workspace.open_on_output.is_some()
                    {
                        ctx.emit_error(DecodeError::unexpected(
                            node,
                            "workspace",
                            "sway-output-assignment and open-on-output are mutually exclusive",
                        ));
                    }
                    config.borrow_mut().workspaces.push(workspace);
                }
                "mode" => {
                    let part = BindingMode::decode_node(node, ctx)?;
                    if part.name.is_empty() {
                        ctx.emit_error(DecodeError::unexpected(
                            &node.node_name,
                            "mode",
                            "mode name must not be empty",
                        ));
                        continue;
                    }
                    let mut config = config.borrow_mut();
                    let binds = if part.name == "default" {
                        &mut config.binds
                    } else if let Some(mode) = config
                        .binding_modes
                        .iter_mut()
                        .find(|mode| mode.name == part.name)
                    {
                        mode.pango_markup |= part.pango_markup;
                        &mut mode.binds
                    } else {
                        config.binding_modes.push(part);
                        continue;
                    };
                    binds.merge(part.binds);
                }

                // Single-part sections.
                "binds" => {
                    let part = Binds::decode_node(node, ctx)?;
                    // Later includes and sections replace conflicting binds,
                    // matching sway's binding insertion behavior.
                    config.borrow_mut().binds.merge(part);
                }
                "environment" => {
                    let part = Environment::decode_node(node, ctx)?;
                    config.borrow_mut().environment.0.extend(part.0);
                }

                "prefer-no-csd" => {
                    config.borrow_mut().prefer_no_csd = Flag::decode_node(node, ctx)?.0
                }

                "popup-during-fullscreen" => {
                    config.borrow_mut().popup_during_fullscreen =
                        PopupDuringFullscreen::decode_node(node, ctx)?
                }

                "focus-on-window-activation" => {
                    config.borrow_mut().focus_on_window_activation =
                        parse_arg_node("focus-on-window-activation", node, ctx)?
                }

                "urgent-timeout-ms" => {
                    config.borrow_mut().urgent_timeout_ms =
                        parse_arg_node("urgent-timeout-ms", node, ctx)?
                }

                "screenshot-path" => {
                    let part = knuffel::Decode::decode_node(node, ctx)?;
                    config.borrow_mut().screenshot_path = part;
                }

                "layout" => {
                    let mut part = LayoutPart::decode_node(node, ctx)?;

                    // Preserve the behavior we'd always had for the border section:
                    // - `layout {}` gives border = off
                    // - `layout { border {} }` gives border = on
                    // - `layout { border { off } }` gives border = off
                    //
                    // This behavior is inconsistent with the rest of the config where adding an
                    // empty section generally doesn't change the outcome. Particularly, shadows
                    // are also disabled by default (like borders), and they always had an `on`
                    // instead of an `off` for this reason, so that writing `layout { shadow {} }`
                    // still results in shadow = off, as it should.
                    //
                    // Unfortunately, the default config has always had wording that heavily
                    // implies that `layout { border {} }` enables the borders. This wording is
                    // sure to be present in a lot of users' configs by now, which we can't change.
                    //
                    // Another way to make things consistent would be to default borders to on.
                    // However, that is annoying because it would mean changing many tests that
                    // rely on borders being off by default. This would also contradict the
                    // intended default borders value (off).
                    //
                    // So, let's just work around the problem here, preserving the original
                    // behavior.
                    if recursion == 0 {
                        if let Some(border) = part.border.as_mut() {
                            if !border.on && !border.off {
                                border.on = true;
                            }
                        }
                    }

                    if let Some(titlebar) = &part.titlebar {
                        let mut merged = config.borrow().layout.titlebar.clone();
                        merged.merge_with(titlebar);
                        if merged.horizontal_padding.min(merged.vertical_padding)
                            < f64::from(merged.border_thickness)
                        {
                            ctx.emit_error(DecodeError::unexpected(
                                node,
                                "layout",
                                "titlebar padding cannot be smaller than border thickness",
                            ));
                        }
                    }

                    config.borrow_mut().layout.merge_with(&part);
                }

                "recent-windows" => {
                    let part = RecentWindowsPart::decode_node(node, ctx)?;

                    let mut config = config.borrow_mut();

                    // When an MRU binds section is encountered for the first time, clear out the
                    // default MRU binds.
                    if !saw_mru_binds.get() && part.binds.is_some() {
                        saw_mru_binds.set(true);
                        config.recent_windows.binds.clear();
                    }

                    config.recent_windows.merge_with(&part);
                }

                "include" => {
                    // Parse the path argument
                    let mut iter_args = node.arguments.iter();
                    let path_val = iter_args.next().ok_or_else(|| {
                        DecodeError::missing(
                            node,
                            "additional argument for include path is required",
                        )
                    })?;
                    let path: PathBuf = knuffel::traits::DecodeScalar::decode(path_val, ctx)?;

                    // Check for extra arguments
                    if let Some(val) = iter_args.next() {
                        ctx.emit_error(DecodeError::unexpected(
                            &val.literal,
                            "argument",
                            "unexpected argument",
                        ));
                    }

                    // Parse the optional property
                    let mut optional = false;
                    for (name, val) in &node.properties {
                        match &***name {
                            "optional" => {
                                optional = knuffel::traits::DecodeScalar::decode(val, ctx)?;
                            }
                            name_str => {
                                ctx.emit_error(DecodeError::unexpected(
                                    name,
                                    "property",
                                    format!("unexpected property `{}`", name_str.escape_default()),
                                ));
                            }
                        }
                    }

                    // Check for unexpected children
                    for child in node.children() {
                        ctx.emit_error(DecodeError::unexpected(
                            child,
                            "node",
                            format!("unexpected node `{}`", child.node_name.escape_default()),
                        ));
                    }

                    // We use DecodeError::Missing throughout this block because it results in the
                    // least confusing error messages while still allowing to provide a span.

                    // Expand ~ into the home dir
                    let path = if let Ok(rest) = path.strip_prefix("~") {
                        let Some(home) = std::env::home_dir() else {
                            ctx.emit_error(DecodeError::missing(
                                node,
                                format!("error retrieving home directory to expand {path:?}"),
                            ));
                            continue;
                        };

                        home.join(rest)
                    } else {
                        // Otherwise, use the current include base dir
                        let base = ctx.get::<BasePath>().unwrap();
                        base.0.join(path)
                    };

                    let recursion = ctx.get::<Recursion>().unwrap().0 + 1;
                    if recursion == RECURSION_LIMIT {
                        ctx.emit_error(DecodeError::missing(
                            node,
                            format!(
                                "reached the recursion limit; \
                                 includes cannot be {RECURSION_LIMIT} levels deep"
                            ),
                        ));
                        continue;
                    }

                    let Some(filename) = path.file_name().and_then(OsStr::to_str) else {
                        ctx.emit_error(DecodeError::missing(
                            node,
                            "include path doesn't have a valid file name",
                        ));
                        continue;
                    };
                    let base = path.parent().map(Path::to_path_buf).unwrap_or_default();

                    // Check for recursive include for a nicer error message.
                    let mut include_stack = ctx.get::<IncludeStack>().unwrap().0.clone();
                    if !include_stack.insert(path.to_path_buf()) {
                        ctx.emit_error(DecodeError::missing(
                            node,
                            "recursive include (file includes itself)",
                        ));
                        continue;
                    }

                    // Store even if the include fails to read or parse, so it gets watched.
                    includes.borrow_mut().0.push(path.to_path_buf());

                    match fs::read_to_string(&path) {
                        Ok(text) => {
                            // Try to get filename relative to the root base config folder for
                            // clearer error messages.
                            let root_base = &ctx.get::<RootBase>().unwrap().0;
                            // Failing to strip prefix usually means absolute path; show it in full.
                            let relative_path = path.strip_prefix(root_base).ok().unwrap_or(&path);
                            let filename = relative_path.to_str().unwrap_or(filename);

                            let part = knuffel::parse_with_context::<
                                ConfigPart,
                                knuffel::span::Span,
                                _,
                            >(filename, &text, |ctx| {
                                ctx.set(BasePath(base));
                                ctx.set(RootBase(root_base.clone()));
                                ctx.set(Recursion(recursion));
                                ctx.set(includes.clone());
                                ctx.set(include_errors.clone());
                                ctx.set(IncludeStack(include_stack));
                                ctx.set(SawMruBinds(saw_mru_binds.clone()));
                                ctx.set(config.clone());
                            });

                            match part {
                                Ok(_) => {}
                                Err(err) => {
                                    include_errors.borrow_mut().0.push(err);

                                    ctx.emit_error(DecodeError::missing(
                                        node,
                                        "failed to parse included config",
                                    ));
                                }
                            }
                        }
                        Err(err) => {
                            if optional && err.kind() == std::io::ErrorKind::NotFound {
                                // Warn about missing optional includes
                                warn!("optional include not found: {path:?}");
                            } else {
                                // Report all other errors normally
                                ctx.emit_error(DecodeError::missing(
                                    node,
                                    format!("failed to read included config from {path:?}: {err}"),
                                ));
                            }
                        }
                    }
                }

                name => {
                    ctx.emit_error(DecodeError::unexpected(
                        node,
                        "node",
                        format!("unexpected node `{}`", name.escape_default()),
                    ));
                }
            }
        }

        Ok(Self)
    }
}

impl Config {
    pub fn load_default() -> Self {
        let res = Config::parse(
            Path::new("default-config.kdl"),
            include_str!("../../resources/default-config.kdl"),
        );

        // Includes in the default config can break its parsing at runtime.
        assert!(
            res.includes.is_empty(),
            "default config must not have includes",
        );

        res.config.unwrap()
    }

    pub fn load(path: &Path) -> ConfigParseResult<Self, miette::Report> {
        let contents = match fs::read_to_string(path) {
            Ok(x) => x,
            Err(err) => {
                return ConfigParseResult::from_err(
                    miette!(err).context(format!("error reading {path:?}")),
                );
            }
        };

        Self::parse(path, &contents).map_config_res(|res| {
            let config = res.context("error parsing")?;
            debug!("loaded config from {path:?}");
            Ok(config)
        })
    }

    pub fn parse(path: &Path, text: &str) -> ConfigParseResult<Self, ConfigIncludeError> {
        let base = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let filename = path
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("config.kdl");

        let config = Rc::new(RefCell::new(Config::default()));
        let includes = Rc::new(RefCell::new(Includes(Vec::new())));
        let include_errors = Rc::new(RefCell::new(IncludeErrors(Vec::new())));
        let include_stack = HashSet::from([path.to_path_buf()]);

        let part = knuffel::parse_with_context::<ConfigPart, knuffel::span::Span, _>(
            filename,
            text,
            |ctx| {
                ctx.set(BasePath(base.clone()));
                ctx.set(RootBase(base));
                ctx.set(Recursion(0));
                ctx.set(includes.clone());
                ctx.set(include_errors.clone());
                ctx.set(IncludeStack(include_stack));
                ctx.set(SawMruBinds(Rc::new(Cell::new(false))));
                ctx.set(config.clone());
            },
        );

        let includes = includes.take().0;
        let include_errors = include_errors.take().0;
        let config = part
            .map(|_| config.take())
            .map_err(move |err| ConfigIncludeError {
                main: err,
                includes: include_errors,
            });

        ConfigParseResult { config, includes }
    }

    pub fn parse_mem(text: &str) -> Result<Self, ConfigIncludeError> {
        Self::parse(Path::new("config.kdl"), text).config
    }
}
