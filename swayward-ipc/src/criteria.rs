use std::str::FromStr;

use pcre2::bytes::{Regex, RegexBuilder};

#[derive(Debug, Clone)]
pub enum Pattern {
    Regex(Regex),
    Focused,
}

impl Pattern {
    fn parse(value: &str) -> Result<Self, String> {
        if value == "__focused__" {
            Ok(Self::Focused)
        } else {
            RegexBuilder::new()
                .ucp(true)
                .build(value)
                .map(Self::Regex)
                .map_err(|error| format!("Regex compilation for '{value}' failed: {error}"))
        }
    }

    fn matches(&self, value: Option<&str>, focused: Option<&str>) -> bool {
        let value = value.unwrap_or("");
        match self {
            Self::Regex(regex) => regex.is_match(value.as_bytes()).unwrap_or(false),
            Self::Focused => focused.is_some_and(|focused| value == focused),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Criteria {
    title: Option<Pattern>,
    shell: Option<Pattern>,
    app_id: Option<Pattern>,
    con_mark: Option<Pattern>,
    con_id: Option<u64>,
    urgent: Option<Urgent>,
    workspace: Option<Pattern>,
    floating: bool,
    tiling: bool,
    all: bool,
    pid: Option<u32>,
    sandbox_engine: Option<Pattern>,
    sandbox_app_id: Option<Pattern>,
    sandbox_instance_id: Option<Pattern>,
    tag: Option<Pattern>,
}

#[derive(Debug, Clone, Copy)]
pub enum Urgent {
    Latest,
    Oldest,
}

#[derive(Default)]
pub struct WindowInfo<'a> {
    pub title: Option<&'a str>,
    pub shell: Option<&'a str>,
    pub app_id: Option<&'a str>,
    pub marks: &'a [String],
    pub con_id: u64,
    pub urgent_since: Option<std::time::Duration>,
    pub workspace: Option<&'a str>,
    pub floating: bool,
    pub pid: Option<u32>,
    pub sandbox_engine: Option<&'a str>,
    pub sandbox_app_id: Option<&'a str>,
    pub sandbox_instance_id: Option<&'a str>,
    pub tag: Option<&'a str>,
}

impl Criteria {
    pub fn matches_unmapped(
        &self,
        title: Option<&str>,
        app_id: Option<&str>,
        pid: Option<u32>,
    ) -> bool {
        self.matches(
            &WindowInfo {
                title,
                shell: Some("xdg_shell"),
                app_id,
                pid,
                ..Default::default()
            },
            &WindowInfo::default(),
        )
    }

    pub fn urgent(&self) -> Option<Urgent> {
        self.urgent
    }

    pub fn matches_container(&self, con_id: u64, marks: &[String]) -> bool {
        (self.con_mark.is_some() || self.con_id.is_some())
            && self.title.is_none()
            && self.shell.is_none()
            && self.app_id.is_none()
            && self.urgent.is_none()
            && self.workspace.is_none()
            && !self.floating
            && !self.tiling
            && self.pid.is_none()
            && self.sandbox_engine.is_none()
            && self.sandbox_app_id.is_none()
            && self.sandbox_instance_id.is_none()
            && self.tag.is_none()
            && self
                .con_mark
                .as_ref()
                .is_none_or(|pattern| marks.iter().any(|mark| pattern.matches(Some(mark), None)))
            && self.con_id.is_none_or(|id| id == con_id)
    }

    pub fn parse(raw: &str, focused_con_id: Option<u64>) -> Result<Self, String> {
        let body = raw
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
            .ok_or_else(|| "No criteria".to_owned())?;
        let pairs = parse_pairs(body)?;
        if pairs.is_empty() {
            return Err("Criteria is empty".into());
        }
        let mut criteria = Self::default();
        for (name, value) in pairs {
            let pattern = || {
                value
                    .as_deref()
                    .ok_or_else(|| format!("Token '{name}' requires a value"))
                    .and_then(Pattern::parse)
            };
            match name.as_str() {
                "all" if value.is_none() => criteria.all = true,
                "floating" if value.is_none() => criteria.floating = true,
                "tiling" if value.is_none() => criteria.tiling = true,
                "title" => criteria.title = Some(pattern()?),
                "shell" => criteria.shell = Some(pattern()?),
                "app_id" => criteria.app_id = Some(pattern()?),
                "con_mark" => criteria.con_mark = Some(pattern()?),
                name @ ("class" | "instance" | "window_role") => {
                    return Err(format!("X11-only criterion '{name}' is unsupported"))
                }
                "workspace" => criteria.workspace = Some(pattern()?),
                "sandbox_engine" => criteria.sandbox_engine = Some(pattern()?),
                "sandbox_app_id" => criteria.sandbox_app_id = Some(pattern()?),
                "sandbox_instance_id" => criteria.sandbox_instance_id = Some(pattern()?),
                "tag" => criteria.tag = Some(pattern()?),
                "con_id" => {
                    let value = required(&name, value.as_deref())?;
                    criteria.con_id = Some(if value == "__focused__" {
                        focused_con_id.unwrap_or(0)
                    } else {
                        value.parse().map_err(|_| {
                            "The value for 'con_id' should be '__focused__' or numeric".to_owned()
                        })?
                    });
                }
                "id" => return Err("X11-only criterion 'id' is unsupported".into()),
                "pid" => {
                    criteria.pid = Some(number::<u32>(&name, required(&name, value.as_deref())?)?)
                }
                "window_type" => {
                    return Err("X11-only criterion 'window_type' is unsupported".into())
                }
                "urgent" => {
                    criteria.urgent = Some(match required(&name, value.as_deref())? {
                        "latest" | "newest" | "last" | "recent" => Urgent::Latest,
                        "oldest" | "first" => Urgent::Oldest,
                        _ => return Err("The value for 'urgent' must be 'first', 'last', 'latest', 'newest', 'oldest' or 'recent'".into()),
                    });
                }
                "all" => criteria.all = true,
                "floating" => criteria.floating = true,
                "tiling" => criteria.tiling = true,
                _ => return Err(format!("Token '{name}' is not recognized")),
            }
        }
        Ok(criteria)
    }

    pub fn matches(&self, window: &WindowInfo<'_>, focused: &WindowInfo<'_>) -> bool {
        let pattern = |wanted: &Option<Pattern>, value, focused_value| {
            wanted
                .as_ref()
                .is_none_or(|p| p.matches(value, focused_value))
        };
        pattern(&self.title, window.title, focused.title)
            && pattern(&self.shell, window.shell, focused.shell)
            && pattern(&self.app_id, window.app_id, focused.app_id)
            && self
                .con_mark
                .as_ref()
                .is_none_or(|p| window.marks.iter().any(|mark| p.matches(Some(mark), None)))
            && self.con_id.is_none_or(|id| id == window.con_id)
            && (!self.floating || window.floating)
            && (!self.tiling || !window.floating)
            && self.urgent.is_none_or(|_| window.urgent_since.is_some())
            && pattern(&self.workspace, window.workspace, focused.workspace)
            && self.pid.is_none_or(|pid| Some(pid) == window.pid)
            && self.sandbox_engine.as_ref().is_none_or(|wanted| {
                window
                    .sandbox_engine
                    .is_some_and(|value| wanted.matches(Some(value), focused.sandbox_engine))
            })
            && self.sandbox_app_id.as_ref().is_none_or(|wanted| {
                window
                    .sandbox_app_id
                    .is_some_and(|value| wanted.matches(Some(value), focused.sandbox_app_id))
            })
            && self.sandbox_instance_id.as_ref().is_none_or(|wanted| {
                window
                    .sandbox_instance_id
                    .is_some_and(|value| wanted.matches(Some(value), focused.sandbox_instance_id))
            })
            && self.tag.as_ref().is_none_or(|wanted| {
                window
                    .tag
                    .is_some_and(|value| wanted.matches(Some(value), focused.tag))
            })
    }
}

fn required<'a>(name: &str, value: Option<&'a str>) -> Result<&'a str, String> {
    value.ok_or_else(|| format!("Token '{name}' requires a value"))
}

fn number<T: FromStr>(name: &str, value: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("The value for '{name}' should be numeric"))
}

fn parse_pairs(input: &str) -> Result<Vec<(String, Option<String>)>, String> {
    let mut chars = input.char_indices().peekable();
    let mut pairs = Vec::new();
    while let Some((_, ch)) = chars.peek().copied() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }
        let start = chars.peek().unwrap().0;
        while chars
            .peek()
            .is_some_and(|(_, ch)| ch.is_ascii_lowercase() || *ch == '_')
        {
            chars.next();
        }
        let end = chars.peek().map_or(input.len(), |(index, _)| *index);
        if end == start {
            return Err("Invalid criteria token".into());
        }
        let name = input[start..end].to_owned();
        while chars.peek().is_some_and(|(_, ch)| ch.is_whitespace()) {
            chars.next();
        }
        let value = if chars.peek().is_some_and(|(_, ch)| *ch == '=') {
            chars.next();
            while chars.peek().is_some_and(|(_, ch)| ch.is_whitespace()) {
                chars.next();
            }
            let quoted = chars.peek().is_some_and(|(_, ch)| *ch == '"');
            if quoted {
                chars.next();
            }
            let mut value = String::new();
            let mut escaped = false;
            loop {
                let Some((_, ch)) = chars.next() else {
                    if quoted {
                        return Err("Quote mismatch in criteria".into());
                    }
                    break;
                };
                if escaped {
                    if ch != '"' {
                        value.push('\\');
                    }
                    value.push(ch);
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if (quoted && ch == '"') || (!quoted && ch.is_whitespace()) {
                    // A quoted value ends at its closing quote; a bare value ends at whitespace.
                    break;
                } else {
                    value.push(ch);
                }
            }
            Some(value)
        } else {
            None
        };
        pairs.push((name, value));
    }
    Ok(pairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcre2_lookaround_pattern_matches() {
        let criteria = Criteria::parse(r#"[title="^(?!Firefox).*$"]"#, None).unwrap();
        let matching = WindowInfo {
            title: Some("Alacritty"),
            ..Default::default()
        };
        let excluded = WindowInfo {
            title: Some("Firefox"),
            ..Default::default()
        };

        assert!(criteria.matches(&matching, &WindowInfo::default()));
        assert!(!criteria.matches(&excluded, &WindowInfo::default()));
    }

    #[test]
    fn quoted_regex_preserves_pcre2_escapes() {
        let criteria = Criteria::parse(
            r#"[title="^say \"hi\" \w \d+$" app_id="^org\.example\.App$"]"#,
            None,
        )
        .unwrap();
        let matching = WindowInfo {
            title: Some("say \"hi\" ä 3"),
            app_id: Some("org.example.App"),
            ..Default::default()
        };
        let digit_escape_must_not_become_literal_d = WindowInfo {
            title: Some("say \"hi\" ä ddd"),
            app_id: Some("org.example.App"),
            ..Default::default()
        };
        let escaped_dot_must_not_become_wildcard = WindowInfo {
            title: Some("say \"hi\" ä 3"),
            app_id: Some("orgXexampleXApp"),
            ..Default::default()
        };

        assert!(criteria.matches(&matching, &WindowInfo::default()));
        assert!(!criteria.matches(
            &digit_escape_must_not_become_literal_d,
            &WindowInfo::default()
        ));
        assert!(!criteria.matches(
            &escaped_dot_must_not_become_wildcard,
            &WindowInfo::default()
        ));
    }

    #[test]
    fn container_match_rejects_view_criteria() {
        let criteria = Criteria::parse("[con_id=42 app_id=doesnotmatch]", None).unwrap();

        assert!(!criteria.matches_container(42, &[]));
    }

    #[test]
    fn numeric_criteria_errors_match_sway_verbatim() {
        assert_eq!(
            Criteria::parse("[con_id=nope]", None).unwrap_err(),
            "The value for 'con_id' should be '__focused__' or numeric"
        );
        assert_eq!(
            Criteria::parse("[pid=nope]", None).unwrap_err(),
            "The value for 'pid' should be numeric"
        );
    }

    #[test]
    fn valued_flag_criteria_keep_sways_presence_only_semantics() {
        let tiled = WindowInfo::default();
        let floating = WindowInfo {
            floating: true,
            ..Default::default()
        };
        let focused = WindowInfo::default();

        let all = Criteria::parse("[all=ignored]", None).unwrap();
        assert!(all.matches(&tiled, &focused));
        assert!(all.matches(&floating, &focused));

        let floating_only = Criteria::parse("[floating=ignored]", None).unwrap();
        assert!(floating_only.matches(&floating, &focused));
        assert!(!floating_only.matches(&tiled, &focused));

        let tiling_only = Criteria::parse("[tiling=ignored]", None).unwrap();
        assert!(tiling_only.matches(&tiled, &focused));
        assert!(!tiling_only.matches(&floating, &focused));
    }

    #[test]
    fn focused_patterns_are_case_sensitive_like_sway() {
        let criteria = Criteria::parse(
            r#"[title="__focused__" app_id="__focused__" shell="__focused__" workspace="__focused__"]"#,
            None,
        )
        .unwrap();
        let focused = WindowInfo {
            title: Some("Editor"),
            app_id: Some("org.example.Editor"),
            shell: Some("xdg_shell"),
            workspace: Some("Main"),
            ..Default::default()
        };
        let matching = WindowInfo {
            title: Some("Editor"),
            app_id: Some("org.example.Editor"),
            shell: Some("xdg_shell"),
            workspace: Some("Main"),
            ..Default::default()
        };
        let wrong_case = WindowInfo {
            title: Some("editor"),
            ..matching
        };

        assert!(criteria.matches(&matching, &focused));
        assert!(!criteria.matches(&wrong_case, &focused));
    }

    #[test]
    fn absent_sandbox_metadata_does_not_match_an_empty_regex() {
        for name in ["sandbox_engine", "sandbox_app_id", "sandbox_instance_id"] {
            let criteria = Criteria::parse(&format!(r#"[{name}="^$"]"#), None).unwrap();
            assert!(
                !criteria.matches(&WindowInfo::default(), &WindowInfo::default()),
                "{name}"
            );
        }
    }

    #[test]
    fn x11_only_criteria_fail_loud() {
        for name in ["class", "instance", "id", "window_role", "window_type"] {
            assert_eq!(
                Criteria::parse(&format!("[{name}=value]"), None).unwrap_err(),
                format!("X11-only criterion '{name}' is unsupported")
            );
        }
    }

    #[test]
    fn absent_tag_does_not_match_an_empty_regex() {
        let criteria = Criteria::parse(r#"[tag="^$"]"#, None).unwrap();
        assert!(!criteria.matches(&WindowInfo::default(), &WindowInfo::default()));
    }

    #[test]
    fn focused_tag_is_case_sensitive() {
        let criteria = Criteria::parse(r#"[tag="__focused__"]"#, None).unwrap();
        let focused = WindowInfo {
            tag: Some("Editor"),
            ..Default::default()
        };
        assert!(criteria.matches(&focused, &focused));
        assert!(!criteria.matches(
            &WindowInfo {
                tag: Some("editor"),
                ..Default::default()
            },
            &focused,
        ));
    }

    #[test]
    fn regex_and_focused_patterns_match() {
        let criteria =
            Criteria::parse(r#"[app_id="^fire" title="__focused__" floating]"#, None).unwrap();
        let focused = WindowInfo {
            title: Some("Editor"),
            ..Default::default()
        };
        let window = WindowInfo {
            app_id: Some("firefox"),
            title: Some("Editor"),
            floating: true,
            ..Default::default()
        };
        assert!(criteria.matches(&window, &focused));
    }
}
