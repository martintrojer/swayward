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
            Self::Focused => focused.is_some_and(|focused| value.eq_ignore_ascii_case(focused)),
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
    id: Option<u64>,
    class: Option<Pattern>,
    instance: Option<Pattern>,
    window_role: Option<Pattern>,
    window_type: Option<String>,
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
enum Urgent {
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
    pub id: Option<u64>,
    pub class: Option<&'a str>,
    pub instance: Option<&'a str>,
    pub window_role: Option<&'a str>,
    pub window_type: Option<&'a str>,
    pub urgent: bool,
    pub workspace: Option<&'a str>,
    pub floating: bool,
    pub pid: Option<u32>,
    pub sandbox_engine: Option<&'a str>,
    pub sandbox_app_id: Option<&'a str>,
    pub sandbox_instance_id: Option<&'a str>,
    pub tag: Option<&'a str>,
}

impl Criteria {
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
                "class" => criteria.class = Some(pattern()?),
                "instance" => criteria.instance = Some(pattern()?),
                "window_role" => criteria.window_role = Some(pattern()?),
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
                        number(name.as_str(), value)?
                    });
                }
                "id" => criteria.id = Some(number(&name, required(&name, value.as_deref())?)?),
                "pid" => {
                    criteria.pid = Some(number::<u32>(&name, required(&name, value.as_deref())?)?)
                }
                "window_type" => {
                    let value = required(&name, value.as_deref())?.to_ascii_lowercase();
                    if !matches!(
                        value.as_str(),
                        "normal"
                            | "dialog"
                            | "utility"
                            | "toolbar"
                            | "splash"
                            | "menu"
                            | "dropdown_menu"
                            | "popup_menu"
                            | "tooltip"
                            | "notification"
                    ) {
                        return Err(format!("Invalid window type '{value}'"));
                    }
                    criteria.window_type = Some(value);
                }
                "urgent" => {
                    criteria.urgent = Some(match required(&name, value.as_deref())? {
                        "latest" | "newest" | "last" | "recent" => Urgent::Latest,
                        "oldest" | "first" => Urgent::Oldest,
                        _ => return Err("The value for 'urgent' must be 'first', 'last', 'latest', 'newest', 'oldest' or 'recent'".into()),
                    });
                }
                "all" | "floating" | "tiling" => {
                    return Err(format!("Token '{name}' does not accept a value"))
                }
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
            && self.id.is_none_or(|id| Some(id) == window.id)
            && pattern(&self.class, window.class, focused.class)
            && pattern(&self.instance, window.instance, focused.instance)
            && pattern(&self.window_role, window.window_role, focused.window_role)
            && self
                .window_type
                .as_deref()
                .is_none_or(|kind| Some(kind) == window.window_type)
            && (!self.floating || window.floating)
            && (!self.tiling || !window.floating)
            && self.urgent.is_none_or(|_| window.urgent)
            && pattern(&self.workspace, window.workspace, focused.workspace)
            && self.pid.is_none_or(|pid| Some(pid) == window.pid)
            && pattern(
                &self.sandbox_engine,
                window.sandbox_engine,
                focused.sandbox_engine,
            )
            && pattern(
                &self.sandbox_app_id,
                window.sandbox_app_id,
                focused.sandbox_app_id,
            )
            && pattern(
                &self.sandbox_instance_id,
                window.sandbox_instance_id,
                focused.sandbox_instance_id,
            )
            && pattern(&self.tag, window.tag, focused.tag)
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
    fn regex_and_focused_patterns_match() {
        let criteria =
            Criteria::parse(r#"[app_id="^fire" title="__focused__" floating]"#, None).unwrap();
        let focused = WindowInfo {
            title: Some("Editor"),
            ..Default::default()
        };
        let window = WindowInfo {
            app_id: Some("firefox"),
            title: Some("editor"),
            floating: true,
            ..Default::default()
        };
        assert!(criteria.matches(&window, &focused));
    }
}
