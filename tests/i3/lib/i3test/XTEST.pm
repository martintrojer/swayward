package i3test::XTEST;
use strict;
use warnings;
use Exporter ();
use JSON::PP qw(decode_json encode_json);
use IO::Socket::UNIX;

our @ISA = qw(Exporter);
our @EXPORT = qw(
    xtest_sync_with_i3
    xtest_key_press
    xtest_key_release
    set_xkb_group
    xtest_button_press
    xtest_button_release
);

sub _control {
    my ($request) = @_;
    my $socket = IO::Socket::UNIX->new(Peer => $ENV{SWAYWARD_TEST_CONTROL})
        or die "connect $ENV{SWAYWARD_TEST_CONTROL}: $!";
    print {$socket} encode_json($request), "\n";
    my $reply = <$socket>;
    defined($reply) or die "test control closed without a reply\n";
    my $decoded = decode_json($reply);
    $decoded->{success} or die(($decoded->{error} // 'input injection failed') . "\n");
}

sub xtest_button_press {
    my ($button, $x, $y) = @_;
    $i3test::x->root->warp_pointer($x, $y);
    if ($button >= 4 && $button <= 7) {
        my ($horizontal, $vertical) = (0, 0);
        $vertical = $button == 4 ? -120 : 120 if $button <= 5;
        $horizontal = $button == 6 ? -120 : 120 if $button >= 6;
        _control({
            action => 'pointer_axis',
            horizontal_v120 => $horizontal,
            vertical_v120 => $vertical,
        });
    } else {
        my %codes = (1 => 0x110, 2 => 0x112, 3 => 0x111, 8 => 0x113, 9 => 0x114);
        _control({ action => 'pointer_button', button => $codes{$button}, pressed => JSON::PP::true });
    }
}

sub xtest_button_release {
    my ($button, $x, $y) = @_;
    return if $button >= 4 && $button <= 7;
    $i3test::x->root->warp_pointer($x, $y);
    my %codes = (1 => 0x110, 2 => 0x112, 3 => 0x111, 8 => 0x113, 9 => 0x114);
    _control({ action => 'pointer_button', button => $codes{$button}, pressed => JSON::PP::false });
}

sub xtest_key_press {
    my ($key) = @_;
    _control({ action => 'key_event', key => $key, pressed => JSON::PP::true });
}

sub xtest_key_release {
    my ($key) = @_;
    _control({ action => 'key_event', key => $key, pressed => JSON::PP::false });
}

sub set_xkb_group {
    my ($group) = @_;
    _control({ action => 'set_xkb_group', group => $group });
}

sub xtest_sync_with_i3 { i3test::sync_with_i3() }

1;
