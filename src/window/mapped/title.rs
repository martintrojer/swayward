fn format_title(
    format: &str,
    title: &str,
    app_id: &str,
    shell: &str,
    sandbox_engine: Option<&str>,
    sandbox_app_id: Option<&str>,
    sandbox_instance_id: Option<&str>,
) -> String {
    const PLACEHOLDERS: [(&str, usize); 8] = [
        ("%sandbox_instance_id", 7),
        ("%sandbox_engine", 5),
        ("%sandbox_app_id", 6),
        ("%instance", 3),
        ("%app_id", 1),
        ("%title", 0),
        ("%class", 2),
        ("%shell", 4),
    ];
    let values = [
        title,
        app_id,
        "",
        "",
        shell,
        sandbox_engine.unwrap_or_default(),
        sandbox_app_id.unwrap_or_default(),
        sandbox_instance_id.unwrap_or_default(),
    ];
    let mut output = String::with_capacity(format.len());
    let mut rest = format;
    while let Some(index) = rest.find('%') {
        output.push_str(&rest[..index]);
        rest = &rest[index..];
        if let Some((placeholder, value)) = PLACEHOLDERS
            .iter()
            .find(|(placeholder, _)| rest.starts_with(placeholder))
        {
            output.push_str(values[*value]);
            rest = &rest[placeholder.len()..];
        } else {
            output.push('%');
            rest = &rest[1..];
        }
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use super::format_title;

    #[test]
    fn title_format_scans_only_the_format_and_expands_all_sway_placeholders() {
        assert_eq!(
            format_title(
                "%title|%app_id|%class|%instance|%shell|%sandbox_app_id|%sandbox_engine|%sandbox_instance_id|%unknown",
                "%app_id",
                "org.example.App",
                "xdg_shell",
                Some("flatpak"),
                Some("org.example.Sandbox"),
                Some("instance-1"),
            ),
            "%app_id|org.example.App|||xdg_shell|org.example.Sandbox|flatpak|instance-1|%unknown"
        );
    }
}

/// Request-size-once logic state.
#[derive(Debug, Clone, Copy)]
enum RequestSizeOnce {
    /// Waiting for configure to be sent with the requested size.
    WaitingForConfigure,
    /// Waiting for the window to commit in response to the configure.
    WaitingForCommit(Serial),
    /// When configuring, use the current window size.
    UseWindowSize,
}
