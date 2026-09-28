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

pub fn set_variable(variables: &mut Vec<(String, String)>, name: String, value: String) {
    match variables.iter_mut().find(|(existing, _)| *existing == name) {
        Some(slot) => slot.1 = value,
        None => variables.push((name, value)),
    }
    variables.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
}
