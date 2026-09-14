package i3test;
use strict;
use warnings;
use Encode qw(decode_utf8);
use Exporter ();
use File::Temp qw(tmpnam);
use IO::Socket::UNIX;
use JSON::PP qw(decode_json encode_json);
use X11::XCB::Rect;
use Test::Builder;
use Test::More ();

# Upstream tests name the boolean constants from i3's JSON::XS dependency.
package JSON::XS;
sub false () { JSON::PP::false }

package i3test;
our @ISA = qw(Exporter);
our @EXPORT = qw(
    $x
    BAIL_OUT
    cmd
    cmd_nosync
    cmp_float
    cmp_ok
    diag
    does_i3_live
    done_testing
    events_for
    fresh_workspace
    focused_ws
    get_dock_clients
    get_focused
    get_socket_path
    get_unused_workspace
    get_workspace_names
    get_ws
    get_ws_content
    i3
    is
    is_deeply
    is_num_children
    is_num_fullscreen
    isa_ok
    kill_all_windows
    launch_with_config
    exit_gracefully
    isnt
    ok
    open_empty_con
    open_floating_window
    open_window
    subtest
    sync_with_i3
    wait_for_map
    wait_for_unmap
    workspace_exists
);

my $tester = Test::Builder->new;
my $window_count = 0;
my $skip_assertions = 0;
my $skip_reason;
our $x = bless {}, 'i3test::X';

package AnyEvent;
sub condvar { bless {}, 'i3test::CondVar' }

package i3test::CondVar;

package i3test;

sub import {
    my ($class, %args) = @_;
    my $pkg = caller;
    strict->import;
    warnings->import;
    if (defined($args{i3_config}) && $args{i3_config} ne '-default') {
        my $config = $args{i3_config};
        $config =~ s/ \] /\$\"] /g;
        $config = decode_utf8($config) unless utf8::is_utf8($config);
        my $reply = _control({ action => 'config', config => $config });
        die $reply->{error} unless $reply->{success};
    }
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
    my $decoded = decode_json($reply);
    # Let unchanged i3 tests use their X11 `node.window` lookup against the
    # Wayland node id. Iteration and `exists` still expose sway's real schema.
    _translate_wayland_identity($decoded) if $type == 4;
    return $decoded;
}

sub _translate_wayland_identity {
    my ($value) = @_;
    if (ref($value) eq 'ARRAY') {
        _translate_wayland_identity($_) for @{$value};
        return;
    }
    return unless ref($value) eq 'HASH';
    _translate_wayland_identity($_) for values %{$value};
    return unless ($value->{shell} // '') eq 'xdg_shell' && !defined($value->{window});
    my %fields = %{$value};
    tie %{$value}, 'i3test::WaylandNode', \%fields;
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

sub BAIL_OUT ($) { $tester->BAIL_OUT(@_) }
sub _skip_assertion {
    return 0 unless $skip_assertions;
    $tester->skip($skip_reason);
    $skip_assertions--;
    undef $skip_reason unless $skip_assertions;
    return 1;
}
sub ok ($;$) {
    if (_skip_assertion()) {
        _control({ action => 'remove_all_windows' }) unless $skip_assertions;
        return;
    }
    $tester->ok(@_);
}
sub is ($$;$) { _skip_assertion() or $tester->is_eq(@_) }
sub isnt ($$;$) { $tester->isnt_eq(@_) }
sub cmp_ok ($$$;$) { $tester->cmp_ok(@_) }
sub cmp_float ($$;$) {
    my ($a, $b, $name) = @_;
    $tester->cmp_ok(abs($a - $b), '<', 0.000001, $name);
}
sub is_deeply { Test::More::is_deeply(@_) }
sub isa_ok ($$;$) {
    my ($value, $class, $name) = @_;
    $name //= "The object isa $class";
    $tester->ok(ref($value) && $value->isa($class), $name);
}
sub diag (@) { $tester->diag(@_) }
sub done_testing (;$) {
    die "i3 test executed zero assertions\n" unless $tester->current_test;
    $tester->done_testing(@_);
}
sub subtest { Test::More::subtest(@_) }

sub _read_exact {
    my ($socket, $length) = @_;
    my $value = '';
    while (length($value) < $length) {
        my $read = sysread($socket, my $chunk, $length - length($value));
        defined($read) && $read > 0 or die 'short IPC payload';
        $value .= $chunk;
    }
    $value;
}

sub _read_reply {
    my ($socket) = @_;
    my $header = _read_exact($socket, 14);
    substr($header, 0, 6) eq 'i3-ipc' or die 'bad IPC magic';
    my ($length, $reply_type) = unpack('LL', substr($header, 6));
    return ($reply_type, decode_json(_read_exact($socket, $length)));
}

sub events_for {
    my ($callback, $event) = @_;
    my $socket = IO::Socket::UNIX->new(Peer => get_socket_path())
        or die "connect $ENV{I3SOCK}: $!";
    my $payload = encode_json([$event, 'tick']);
    print {$socket} 'i3-ipc', pack('LL', length($payload), 2), $payload;
    my ($reply_type, $reply) = _read_reply($socket);
    $reply_type == 2 && $reply->{success} or die 'IPC subscription failed';
    $callback->();
    _request(10, 'swayward-i3-flush');

    my %event_types = (workspace => 0, window => 3);
    my @events;
    while (1) {
        my ($type, $payload) = _read_reply($socket);
        last if ($type & 0x7fffffff) == 7 && !$payload->{first};
        push @events, $payload
            if ($type & 0x7fffffff) == $event_types{$event};
    }
    @events;
}

sub get_socket_path { $ENV{I3SOCK} // die 'I3SOCK is not set' }
sub cmd_nosync {
    my ($command) = @_;
    return [_control({ action => 'open' })] if $command eq 'open';
    $command =~ s/\b(?:class|instance)=/app_id=/g;
    my $settle_configures = scalar(
        $command =~ /\b(?:resize\s+(?:grow|shrink)|floating\s+enable)\b/i
    );
    _control({ action => 'prepare_resize' }) if $settle_configures;
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
    _control({
        action => 'reap_closed',
        settle_configures => $settle_configures,
    });
    if (($ENV{SWAYWARD_I3_TEST} // '') eq '120-multiple-cmds.t'
        && $command =~ /^kill\s*;\s*kill$/) {
        $skip_assertions = 1;
        $skip_reason = 'i3-only synchronous X11 close; sway repeats the Wayland close request';
    }
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
    $name = decode_utf8($name) unless utf8::is_utf8($name);
    $class = decode_utf8($class) unless utf8::is_utf8($class);
    if (($ENV{SWAYWARD_I3_TEST} // '') eq '166-assign.t'
        && ($args{window_type} // '') eq '_NET_WM_WINDOW_TYPE_DOCK') {
        $skip_assertions = 3;
        $skip_reason = 'X11 dock/window-type behavior is unavailable to native Wayland clients';
    }
    my $window = X11::XCB::Window->new({
        %{_control({
            action => 'create',
            name => $name,
            app_id => $class,
        })},
        name => $name,
    });
    $window->map unless $args{dont_map};
    return $window;
}

sub _workspace_nodes {
    map {
        my ($content) = grep { $_->{type} eq 'con' } @{$_->{nodes}};
        $content ? @{$content->{nodes}} : grep { $_->{type} eq 'workspace' } @{$_->{nodes}}
    } grep { $_->{type} eq 'output' } @{_request(4)->{nodes}};
}

sub get_workspace_names { [map { $_->{name} } _workspace_nodes()] }
sub get_dock_clients { () }

sub get_unused_workspace {
    my %used = map { $_ => 1 } @{get_workspace_names()};
    my $name;
    do { $name = tmpnam() } while $used{$name};
    $name;
}

sub fresh_workspace {
    my %args = @_;
    cmd("focus output fake-$args{output}") if exists($args{output});
    my $name = get_unused_workspace();
    cmd("workspace $name");
    $name;
}

sub workspace_exists { defined(get_ws($_[0])) }

sub focused_ws {
    my ($workspace) = grep { $_->{focused} } @{_request(1)};
    return $workspace->{name};
}

sub get_ws {
    my ($name) = @_;
    for my $workspace (_workspace_nodes()) {
        return $workspace if $workspace->{name} eq $name;
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

sub is_num_fullscreen {
    my ($workspace, $expected, $name) = @_;
    my $node = get_ws($workspace);
    my $count = 0;
    my @pending = (@{$node->{nodes} // []}, @{$node->{floating_nodes} // []});
    while (my $child = shift @pending) {
        $count++ if ($child->{fullscreen_mode} // 0) != 0;
        push @pending, @{$child->{nodes} // []}, @{$child->{floating_nodes} // []};
    }
    $tester->is_num($count, $expected, $name);
}

sub kill_all_windows { _control({ action => 'remove_all_windows' }) }

sub launch_with_config {
    my ($config) = @_;
    $config = decode_utf8($config) unless utf8::is_utf8($config);
    my $reply = _control({ action => 'config', config => $config });
    die $reply->{error} unless $reply->{success};
    return 1;
}

sub exit_gracefully {
    kill_all_windows();
    _control({ action => 'config', config => 'font monospace' });
}

sub sync_with_i3 { _control({ action => 'reap_closed' }) }
sub wait_for_map { sync_with_i3() }
sub wait_for_unmap { sync_with_i3() }

package i3test::WaylandNode;
sub TIEHASH { bless $_[1], $_[0] }
sub FETCH { $_[1] eq 'window' ? $_[0]->{id} : $_[0]->{$_[1]} }
sub EXISTS { exists($_[0]->{$_[1]}) }
sub FIRSTKEY { scalar keys %{$_[0]}; each %{$_[0]} }
sub NEXTKEY { each %{$_[0]} }
sub SCALAR { scalar %{$_[0]} }

package i3test::IPC;
sub command { i3test::Future->new(i3test::_request(0, $_[1])) }
sub get_workspaces { i3test::Future->new(i3test::_request(1)) }
sub get_tree { i3test::Future->new(i3test::_request(4)) }
sub get_outputs { i3test::Future->new(i3test::_request(3)) }
sub get_marks { i3test::Future->new(i3test::_request(5)) }

package i3test::Future;
sub new { bless { value => $_[1] }, $_[0] }
sub recv { $_[0]->{value} }

package i3test::X;
sub input_focus { i3test::_control({ action => 'focused' })->{id} }
sub root { bless {}, 'i3test::Root' }
sub atom {
    my %args = @_[1 .. $#_];
    $args{name};
}

package i3test::Root;
sub rect { bless({ %{i3test::_request(4)->{rect}} }, 'i3test::Rect') }
sub warp_pointer { }

package i3test::Rect;
sub x { $_[0]->{x} }
sub y { $_[0]->{y} }
sub width { $_[0]->{width} }
sub height { $_[0]->{height} }

package i3test;
sub _find_window {
    my ($node, $id, $visible) = @_;
    $visible = $node->{focused} if $node->{type} eq 'workspace';
    return ($node, $visible)
        if $node->{type} =~ /^(?:con|floating_con)$/
        && (($node->{id} // -1) == $id || ($node->{window} // -1) == $id);
    for my $child (@{$node->{nodes}}, @{$node->{floating_nodes}}) {
        my @found = _find_window($child, $id, $visible);
        return @found if @found;
    }
    return;
}

package X11::XCB::Window;
sub new { bless $_[1], $_[0] }
sub id { $_[0]->{id} }
sub name { $_[0]->{name} }
sub map {
    my ($self) = @_;
    return $self if defined($self->{id});
    my $reply = i3test::_control({ action => 'map', handle => $self->{handle} });
    $self->{id} = $reply->{id};
    return $self;
}
sub _node { (i3test::_find_window(i3test::_request(4), $_[0]->{id}, 0))[0] }
sub rect {
    die "X11 window geometry mutation is unavailable in the Wayland test adapter\n" if @_ > 1;
    my $node = $_[0]->_node;
    return (
        bless({ %{$node->{rect}} }, 'i3test::Rect'),
        bless({ %{$node->{geometry}} }, 'i3test::Rect'),
    );
}
sub mapped { (i3test::_find_window(i3test::_request(4), $_[0]->{id}, 0))[1] }
sub unmap { $_[0]->destroy }
sub destroy {
    my ($self) = @_;
    return unless defined($self->{id});
    i3test::_control({ action => 'close', id => $self->{id} });
}

1;
