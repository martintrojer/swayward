use std::fs::File;
use std::io::{Seek, SeekFrom};
use std::os::fd::AsFd;

use smithay::reexports::rustix::fs::{ftruncate, memfd_create, MemfdFlags};
use smithay::reexports::rustix::pipe::pipe;

use super::Fixture;

fn gamma_fd(len: usize) -> std::os::fd::OwnedFd {
    let fd = memfd_create("swayward-test-gamma", MemfdFlags::CLOEXEC).unwrap();
    ftruncate(&fd, len as u64).unwrap();
    fd
}

#[test]
fn unreadable_pipe_fails_instead_of_blocking_the_compositor() {
    let mut f = Fixture::new();
    f.add_output(1, (320, 240));
    let client = f.add_client();
    f.double_roundtrip(client);
    let output = f.client(client).output("headless-1");
    let control = f.client(client).gamma_control(&output).proxy.clone();
    f.roundtrip(client);

    let (read, _write) = pipe().unwrap();
    control.set_gamma(read.as_fd());
    f.client(client).connection.flush().unwrap();
    f.roundtrip(client);

    assert!(f.client(client).state.gamma_controls[0].failed);
}

#[test]
fn short_and_oversized_ramps_fail_without_panicking() {
    for len in [10, 14] {
        let mut f = Fixture::new();
        f.add_output(1, (320, 240));
        let client = f.add_client();
        f.double_roundtrip(client);
        let output = f.client(client).output("headless-1");
        let control = f.client(client).gamma_control(&output).proxy.clone();
        f.roundtrip(client);
        assert_eq!(f.client(client).state.gamma_controls[0].gamma_size, Some(2));

        let fd = gamma_fd(len);
        control.set_gamma(fd.as_fd());
        f.client(client).connection.flush().unwrap();
        f.roundtrip(client);

        assert!(f.client(client).state.gamma_controls[0].failed);
    }
}

#[test]
fn exact_ramp_length_is_accepted_and_destroy_releases_exclusivity() {
    let mut f = Fixture::new();
    f.add_output(1, (320, 240));
    let first_client = f.add_client();
    let second_client = f.add_client();
    f.double_roundtrip(first_client);
    f.double_roundtrip(second_client);
    let first_output = f.client(first_client).output("headless-1");
    let second_output = f.client(second_client).output("headless-1");
    let first = f
        .client(first_client)
        .gamma_control(&first_output)
        .proxy
        .clone();
    f.roundtrip(first_client);
    assert_eq!(
        f.client(first_client).state.gamma_controls[0].gamma_size,
        Some(2)
    );
    assert!(!f.client(first_client).state.gamma_controls[0].failed);

    let fd = gamma_fd(12);
    let mut file = File::from(fd);
    file.seek(SeekFrom::End(0)).unwrap();
    first.set_gamma(file.as_fd());
    f.client(first_client).connection.flush().unwrap();
    f.roundtrip(first_client);
    assert!(!f.client(first_client).state.gamma_controls[0].failed);

    first.destroy();
    f.client(first_client).connection.flush().unwrap();
    f.roundtrip(first_client);
    f.client(second_client).gamma_control(&second_output);
    f.roundtrip(second_client);
    assert!(!f.client(second_client).state.gamma_controls[0].failed);
}

#[test]
fn a_second_control_for_the_same_output_fails() {
    let mut f = Fixture::new();
    f.add_output(1, (320, 240));
    let first_client = f.add_client();
    let second_client = f.add_client();
    f.double_roundtrip(first_client);
    f.double_roundtrip(second_client);
    let first_output = f.client(first_client).output("headless-1");
    let second_output = f.client(second_client).output("headless-1");

    f.client(first_client).gamma_control(&first_output);
    f.roundtrip(first_client);
    f.client(second_client).gamma_control(&second_output);
    f.roundtrip(second_client);

    assert!(!f.client(first_client).state.gamma_controls[0].failed);
    assert!(f.client(second_client).state.gamma_controls[0].failed);
}

#[test]
fn removing_an_output_fails_its_control_and_stale_requests_are_safe() {
    let mut f = Fixture::new();
    f.add_output(1, (320, 240));
    let client = f.add_client();
    f.double_roundtrip(client);
    let wl_output = f.client(client).output("headless-1");
    let control = f.client(client).gamma_control(&wl_output).proxy.clone();
    f.roundtrip(client);

    let output = f.niri_output(1);
    f.swayward().remove_output(&output);
    f.roundtrip(client);
    assert!(f.client(client).state.gamma_controls[0].failed);

    let fd = gamma_fd(12);
    control.set_gamma(fd.as_fd());
    f.client(client).connection.flush().unwrap();
    f.roundtrip(client);
}
