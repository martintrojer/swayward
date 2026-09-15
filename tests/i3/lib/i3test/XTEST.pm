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
    i3test::$x->root->warp_pointer($x, $y);
    _control({ action => 'pointer_button', button => $button, pressed => JSON::PP::true });
}

sub xtest_button_release {
    my ($button, $x, $y) = @_;
    i3test::$x->root->warp_pointer($x, $y);
    _control({ action => 'pointer_button', button => $button, pressed => JSON::PP::false });
}

sub xtest_key_press {
    my ($key) = @_;
    _control({ action => 'key_event', key => $key, pressed => JSON::PP::true });
}

sub xtest_key_release {
    my ($key) = @_;
    _control({ action => 'key_event', key => $key, pressed => JSON::PP::false });
}

sub xtest_sync_with_i3 { i3test::sync_with_i3() }

1;
