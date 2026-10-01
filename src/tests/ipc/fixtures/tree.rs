fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(number) if number.is_f64() => "float",
        Value::Number(_) => "integer",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn workspace_names(fixture: &mut Fixture) -> Vec<String> {
    let swayward = fixture.swayward();
    let mut names = describe_workspaces(&swayward.layout, &swayward.global_space)
        .into_iter()
        .map(|workspace| workspace.name)
        .collect::<Vec<_>>();
    names.sort();
    names
}

/// Put one window with `app_id` on each named workspace, leaving the last
/// created workspace focused.
fn windows_on_workspaces(fixture: &mut Fixture, plan: &[(&str, &str)]) {
    let client = fixture.add_client();
    for (workspace, app_id) in plan {
        assert!(
            crate::command::execute(fixture.niri_state(), &format!("workspace {workspace}"))[0]
                .success
        );
        let window = fixture.client(client).create_window();
        window.xdg_toplevel.set_app_id((*app_id).into());
        window.commit();
        let surface = window.surface.clone();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }
}
