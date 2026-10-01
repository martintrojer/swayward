/// Expand sway variables in a command line, as sway does before dispatch.
///
/// Mirrors `do_var_replacement` (`sway/sway/config.c:890-940`):
///
/// - `\$` is not expanded: sway skips past the `$`, and the backslash survives into the argument,
///   where quote stripping removes it.
/// - `$$` collapses to a single `$`.
/// - the first variable whose name prefixes the text wins. `variables` must be sorted longest name
///   first, which is how sway keeps `config->symbols` (`sway/sway/commands/set.c:13-15`), so
///   `$mod2` is not shadowed by `$mod`.
/// - an unknown `$name` is left verbatim (`sway/sway/config.c:935-937`).
///
/// Substitution is textual and single-pass: a value containing `$` is not
/// re-expanded, because sway resumes scanning after the inserted value
/// (`sway/sway/config.c:931`).
pub fn expand_variables(input: &str, variables: &[(String, String)]) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        if byte != b'$' {
            let Some(ch) = input.get(i..).and_then(|tail| tail.chars().next()) else {
                break;
            };
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        // An escaped `$` keeps its backslash; sway leaves both in place here
        // and strips the escape later with the quotes.
        if bytes.get(i.wrapping_sub(1)) == Some(&b'\\') {
            out.push('$');
            i += 1;
            continue;
        }
        // `$$` unescapes to one `$`.
        if bytes.get(i + 1) == Some(&b'$') {
            out.push('$');
            i += 2;
            continue;
        }
        match variables.iter().find(|(name, _)| {
            input
                .get(i..)
                .is_some_and(|tail| tail.starts_with(name.as_str()))
        }) {
            Some((name, value)) => {
                out.push_str(value);
                i += name.len();
            }
            None => {
                out.push('$');
                i += 1;
            }
        }
    }
    out
}

/// Insert or replace a variable and keep sway's longest-name-first order.
pub fn set_variable(variables: &mut Vec<(String, String)>, name: String, value: String) {
    match variables.iter_mut().find(|(existing, _)| *existing == name) {
        Some(slot) => slot.1 = value,
        None => variables.push((name, value)),
    }
    variables.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
}
