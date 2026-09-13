package i3test;
use strict;
use warnings;
use Exporter ();
use File::Temp qw(tmpnam);
use IO::Socket::UNIX;
use JSON::PP qw(decode_json);
use Test::Builder;

our @ISA = qw(Exporter);
our @EXPORT = qw(
    $x
    cmd
    cmd_nosync
    cmp_float
    cmp_ok
    diag
    does_i3_live
    done_testing
    fresh_workspace
    get_focused
    get_socket_path
    get_unused_workspace
    get_ws
    get_ws_content
    i3
    is
    is_deeply
    is_num_children
    isnt
    ok
    open_empty_con
    open_floating_window
    open_window
    sync_with_i3
    wait_for_unmap
    workspace_exists
);

my $tester = Test::Builder->new;
my $window_count = 0;
our $x = bless {}, 'i3test::X';

sub import {
    my ($class, %args) = @_;
    my $pkg = caller;
    strict->import;
    warnings->import;
    $class->export_to_level(1, $class);
}

sub _request {
    my ($type, $payload) = @_;
    $payload //= '';
    my $socket = IO::Socket::UNIX->new(Peer => get_socket_path())
        or die "connect $ENV{I3SOCK}: $!";
    print {$socket} 'i3-ipc', pack('LL', length($payload), $type), $payload;
    read($socket, my $header, 14) == 14 or die 'short IPC header';
    substr($header, 0, 6) eq 'i3-ipc' or die 'bad IPC magic';
    my ($length, $reply_type) = unpack('LL', substr($header, 6));
    $reply_type == $type or die "reply type $reply_type != $type";
    my $reply = '';
    while (length($reply) < $length) {
        my $read = read($socket, my $chunk, $length - length($reply));
        defined($read) && $read > 0 or die 'short IPC payload';
        $reply .= $chunk;
    }
    decode_json($reply);
}

sub _control {
    my ($request) = @_;
    my $socket = IO::Socket::UNIX->new(Peer => $ENV{SWAYWARD_TEST_CONTROL})
        or die "connect $ENV{SWAYWARD_TEST_CONTROL}: $!";
    print {$socket} JSON::PP::encode_json($request), "\n";
    my $reply = <$socket>;
    defined($reply) or die 'test control closed without a reply';
    decode_json($reply);
}

sub ok ($;$) { $tester->ok(@_) }
sub is ($$;$) { $tester->is_eq(@_) }
sub isnt ($$;$) { $tester->isnt_eq(@_) }
sub cmp_ok ($$$;$) { $tester->cmp_ok(@_) }
sub cmp_float ($$;$) {
    my ($a, $b, $name) = @_;
    $tester->cmp_ok(abs($a - $b), '<', 0.000001, $name);
}
sub is_deeply ($$;$) { $tester->is_deeply(@_) }
sub diag (@) { $tester->diag(@_) }
sub done_testing (;$) { $tester->done_testing(@_) }

sub get_socket_path { $ENV{I3SOCK} // die 'I3SOCK is not set' }
sub cmd_nosync {
    my ($command) = @_;
    return [_control({ action => 'open' })] if $command eq 'open';
    my $reply = _request(0, $command);
    # Upstream tests ignore command replies, so a command swayward REJECTS looks
    # identical to one that ran and did nothing. That once hid a parser gap
    # behind an apparent focus bug. Warn loudly instead: the reply is still
    # returned, and no upstream assertion is affected.
    for my $outcome (@{ $reply // [] }) {
        next if ref($outcome) ne 'HASH' || $outcome->{success};
        my $error = $outcome->{error} // 'no error text';
        $tester->diag("swayward rejected `$command`: $error");
    }
    _control({ action => 'reap_closed' });
    $reply;
}
sub cmd { cmd_nosync(@_) }
sub does_i3_live { $tester->ok(defined(_request(4)), 'i3 lives') }

sub i3 { bless {}, 'i3test::IPC' }
sub open_empty_con { _control({ action => 'open' })->{id} }
sub open_floating_window {
    my $window = open_window(@_);
    cmd('[con_id=' . $window->id . '] floating enable');
    $window;
}
sub open_window {
    my %args = @_ == 1 ? %{$_[0]} : @_;
    my $name = $args{name} // 'Window ' . $window_count++;
    my $class = $args{wm_class} // $name;
    return i3test::Window->new(_control({
        action => 'open',
        name => $name,
        app_id => $class,
    }));
}

sub get_workspace_names {
    [map { $_->{name} } map { @{$_->{nodes}} }
        grep { $_->{type} eq 'output' && $_->{name} ne '__i3' }
        @{_request(4)->{nodes}}]
}

sub get_unused_workspace {
    my %used = map { $_ => 1 } @{get_workspace_names()};
    my $name;
    do { $name = tmpnam() } while $used{$name};
    $name;
}

sub fresh_workspace {
    my $name = get_unused_workspace();
    cmd("workspace $name");
    $name;
}

sub workspace_exists { defined(get_ws($_[0])) }

sub get_ws {
    my ($name) = @_;
    for my $output (@{_request(4)->{nodes}}) {
        return $_ for grep { $_->{type} eq 'workspace' && $_->{name} eq $name }
            @{$output->{nodes}};
    }
    return;
}

sub get_ws_content {
    my ($name) = @_;
    my $workspace = get_ws($name);
    return wantarray ? ($workspace->{nodes}, $workspace->{focus}) : $workspace->{nodes};
}

sub get_focused {
    my ($name) = @_;
    my $node = get_ws($name);
    my $focused;
    while (@{$node->{focus}}) {
        $focused = $node->{focus}[0];
        my ($child) = grep { $_->{id} == $focused }
            (@{$node->{nodes}}, @{$node->{floating_nodes}});
        last unless $child;
        $node = $child;
    }
    $focused;
}

sub is_num_children {
    my ($workspace, $expected, $name) = @_;
    my $node = get_ws($workspace);
    $tester->ok(defined($node), "Workspace $workspace exists");
    return $tester->skip('Workspace does not exist') unless $node;
    $tester->is_num(scalar @{$node->{nodes}}, $expected, $name);
}

sub sync_with_i3 { _control({ action => 'reap_closed' }) }
sub wait_for_unmap { sync_with_i3() }

package i3test::IPC;
sub command { i3test::Future->new(i3test::_request(0, $_[1])) }
sub get_workspaces { i3test::Future->new(i3test::_request(1)) }
sub get_tree { i3test::Future->new(i3test::_request(4)) }

package i3test::Future;
sub new { bless { value => $_[1] }, $_[0] }
sub recv { $_[0]->{value} }

package i3test::X;
sub input_focus { i3test::_control({ action => 'focused' })->{id} }

package i3test::Window;
sub new { bless $_[1], $_[0] }
sub id { $_[0]->{id} }
sub unmap { $_[0]->destroy }
sub destroy { i3test::_control({ action => 'close', id => $_[0]->{id} }) }

1;
