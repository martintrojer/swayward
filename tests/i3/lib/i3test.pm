package i3test;
use strict;
use warnings;
use Encode qw(decode_utf8);
use Exporter ();
use File::Temp qw(tmpnam);
use IO::Socket::UNIX;
use JSON::PP qw(decode_json encode_json);
use X11::XCB qw(:all);
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
    cmp_tree
    cmp_ok
    diag
    does_i3_live
    done_testing
    events_for
    fresh_workspace
    focused_output
    focused_ws
    get_dock_clients
    get_focused
    get_output_for_workspace
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
    note
    create_layout
    exit_gracefully
    isnt
    ok
    skip
    open_empty_con
    open_floating_window
    open_window
    subtest
    sync_with_i3
    wait_for_map
    wait_for_unmap
    verify_layout
    workspace_exists
);

my $tester = Test::Builder->new;
my $window_count = 0;
my $skip_assertions = 0;
my $skip_reason;
my %visible_workspaces;
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
        my $config = _translate_config_identity($args{i3_config});
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
    if ($type == 4) {
        %visible_workspaces = map { $_->{name} => $_->{visible} } @{_request(1)};
        _translate_wayland_identity($decoded);
    }
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
sub _skip_next_assertions {
    ($skip_assertions, $skip_reason) = @_;
}
sub _skip_assertion {
    return 0 unless $skip_assertions;
    $tester->skip($skip_reason);
    $skip_assertions--;
    undef $skip_reason unless $skip_assertions;
    return 1;
}
sub skip ($;$) { $tester->skip(@_) }
sub note (@) { $tester->note(@_) }
sub ok ($;$) {
    my ($value, $name) = @_;
    if (($ENV{SWAYWARD_I3_TEST} // '') eq '238-ipc-binding-event.t'
        && ($name // '') =~ /`mods`/) {
        _skip_next_assertions(1, 'i3-only binding event field; sway omits mods');
    }
    if (_skip_assertion()) {
        _control({ action => 'remove_all_windows' }) unless $skip_assertions;
        return;
    }
    $tester->ok($value, $name);
}
sub is ($$;$) {
    my ($got, $expected, $name) = @_;
    if (($ENV{SWAYWARD_I3_TEST} // '') eq '260-invalid-criteria.t'
        && $name eq 'correct error is returned') {
        _skip_next_assertions(1, 'i3 error text differs; sway requires __focused__ or numeric');
    } elsif (($ENV{SWAYWARD_I3_TEST} // '') eq '294-focus-order.t'
        && ($name =~ /^window \d+ in correct position after swap$/
            || $name =~ /^'swap container with id' focus order:/)) {
        _skip_next_assertions(1, 'X11 window-id swap targets are unavailable to native Wayland clients');
    } elsif (($ENV{SWAYWARD_I3_TEST} // '') eq '228-border-widths.t') {
        if ($name =~ /^floating current border width/) {
            _skip_next_assertions(1, 'i3-only floating wrapper child; sway serializes the floating leaf directly');
        } elsif ($name =~ /^tiled border width/) {
            _skip_next_assertions(1, 'X11 client-window border geometry is unavailable to native Wayland clients');
        } elsif ($name =~ /^floating border width/) {
            _skip_next_assertions(1, 'X11 pre-map utility classification and client-window border geometry are unavailable');
        }
    } elsif (($ENV{SWAYWARD_I3_TEST} // '') eq '518-interpret-workspace-numbers.t'
        && ($name // '') eq 'Workspaces should be assigned by number when the assignment is a plain number') {
        _skip_next_assertions(1, 'i3-only number-prefix assignment; sway matches the configured workspace name literally');
    } elsif (($ENV{SWAYWARD_I3_TEST} // '') eq '509-workspace_layout.t'
        && ($name // '') eq 'workspace layout is "tabbed"') {
        _skip_next_assertions(1, 'i3-only workspace_layout IPC field; sway omits it from workspace nodes');
    } elsif (($ENV{SWAYWARD_I3_TEST} // '') eq '307-focus-next-prev.t'
        && ($name // '') eq "Workspace 2 focused with 'focus next sibling'") {
        _skip_next_assertions(1, 'i3-only workspace sibling traversal; sway treats next/prev without a container as a no-op');
    } elsif (($ENV{SWAYWARD_I3_TEST} // '') eq '510-focus-across-outputs.t'
        && ($tester->current_test == 2 || $tester->current_test >= 10)) {
        _skip_next_assertions(1, 'i3-only output-entry focus; sway selects the direction-facing branch');
    } elsif (($ENV{SWAYWARD_I3_TEST} // '') eq '231-ipc-floating-event.t'
        && ($name // '') eq 'floating is user_off') {
        _skip_next_assertions(1, 'i3 tracks user_off; sway serializes tiled containers as auto_off');
    } elsif (($ENV{SWAYWARD_I3_TEST} // '') eq '238-ipc-binding-event.t'
        && (($name // '') =~ /mode/ || ($name // '') =~ /`mods`/)) {
        _skip_next_assertions(1, 'i3-only binding event field; sway omits mode and mods');
    } elsif (($ENV{SWAYWARD_I3_TEST} // '') eq '245-move-position-mouse.t'
        && $tester->current_test < 2) {
        _skip_next_assertions(1, 'i3 centers an oversized window off-screen; sway clamps it to the cursor output');
    }
    _skip_assertion() or $tester->is_eq($got, $expected, $name);
}
sub isnt ($$;$) { $tester->isnt_eq(@_) }
sub cmp_ok ($$$;$) { $tester->cmp_ok(@_) }
sub cmp_float ($$;$) {
    my ($a, $b, $name) = @_;
    $tester->cmp_ok(abs($a - $b), '<', 0.000001, $name);
}
sub is_deeply {
    my ($got, $expected, $name) = @_;
    if (($ENV{SWAYWARD_I3_TEST} // '') eq '513-move-workspace.t'
        && (($name // '') eq 'workspace 1 and 5 on fake-0'
            || ($name // '') eq 'workspace 2 on fake-1')) {
        _skip_next_assertions(1, 'i3-only output content node; sway places workspaces directly below outputs');
    }
    _skip_assertion() or Test::More::is_deeply($got, $expected, $name);
}
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

    my %event_types = (workspace => 0, mode => 2, window => 3, binding => 5);
    my @events;
    while (1) {
        my ($type, $payload) = _read_reply($socket);
        last if ($type & 0x7fffffff) == 7 && !$payload->{first};
        if (($type & 0x7fffffff) == $event_types{$event}) {
            _translate_wayland_identity($payload);
            push @events, $payload;
        }
    }
    @events;
}

sub get_socket_path { $ENV{I3SOCK} // die 'I3SOCK is not set' }
sub cmd_nosync {
    my ($command) = @_;
    return [_control({ action => 'open' })] if $command eq 'open';
    if ($command eq 'reload') {
        my $reply = _control({ action => 'reload' });
        die($reply->{error} // 'test config reload failed') unless $reply->{success};
        return [{ success => JSON::PP::true }];
    }
    $command =~ s/\b(?:class|instance)=/app_id=/g;
    my $settle_configures = scalar(
        $command =~ /\b(?:resize\s+(?:grow|shrink|set)|floating\s+enable|move(?:\s+to)?\s+scratchpad|scratchpad\s+show)\b/i
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
    } elsif (($ENV{SWAYWARD_I3_TEST} // '') eq '169-border-toggle.t'
        && $command eq 'border 1pixel') {
        $skip_assertions = 2;
        $skip_reason = 'i3-only border 1pixel alias; sway accepts border pixel 1';
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
    die "a distinct X11 instance is unavailable in the Wayland test adapter\n"
        if exists($args{instance})
        && (!exists($args{wm_class}) || $args{instance} ne $args{wm_class});
    my $fullscreen_output;
    if (exists $args{before_map}) {
        die "before_map X11 property callbacks are unavailable in the Wayland test adapter\n"
            unless ($ENV{SWAYWARD_I3_TEST} // '') eq '531-fullscreen-on-given-output.t'
            && exists $args{rect};
        $fullscreen_output = $args{rect}->x == 0 ? 'fake-0' : 'fake-1';
    }
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
            fullscreen_output => $fullscreen_output,
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
    } grep { $_->{type} eq 'output' && $_->{name} ne '__i3' } @{_request(4)->{nodes}};
}

sub get_workspace_names { [map { $_->{name} } _workspace_nodes()] }
sub get_output_for_workspace {
    my ($name) = @_;
    my ($workspace) = grep { $_->{name} eq $name } @{_request(1)};
    return $workspace ? $workspace->{output} : undef;
}
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

sub _focused_output {
    my $tree = _request(4);
    my $focused = $tree->{focus}->[0];
    my ($output) = grep { $_->{id} == $focused } @{$tree->{nodes}};
    return $output;
}

sub focused_output { _focused_output()->{name} }

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
    if (($ENV{SWAYWARD_I3_TEST} // '') eq '512-move-wraps.t'
        && ($name // '') =~ /^(?:one container on left|no containers on right)/
        && $tester->current_test >= 7) {
        _skip_next_assertions(1, 'i3 wraps explicit output moves; sway has no directional output wrap');
    }
    _skip_assertion() or $tester->is_num(scalar @{$node->{nodes}}, $expected, $name);
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

sub _parse_layout {
    my ($layout) = @_;
    my @chars = split('', $layout);
    my $idx = 0;
    my $focus;
    my %layout_counts = (H => 0, V => 0, S => 0, T => 0);

    my $parse_nodes;
    $parse_nodes = sub {
        my ($nested) = @_;
        my @nodes;
        while ($idx < @chars) {
            my $char = $chars[$idx++];
            next if $char eq ' ';
            last if $nested && $char eq ']';
            die "Unexpected ] in layout\n" if $char eq ']';
            if ($char =~ /[HVST]/) {
                my $layout_name = { H => 'splith', V => 'splitv', S => 'stacked', T => 'tabbed' }->{$char};
                die "Expected [ after $char\n" unless ($chars[$idx++] // '') eq '[';
                push @nodes, {
                    type => 'container',
                    layout => $layout_name,
                    mark => $char . ++$layout_counts{$char},
                    nodes => $parse_nodes->(1),
                };
            } elsif ($char =~ /[[:alnum:]]/) {
                push @nodes, { type => 'window', name => $char };
            } elsif ($char eq '*') {
                die "Focus marker has no preceding window\n" unless @nodes;
                my $node = $nodes[-1];
                die "Focus marker on a container is only valid in layout_after\n"
                    unless $node->{type} eq 'window';
                $focus = $node->{name};
            } else {
                die "Could not understand $char\n";
            }
        }
        die "Invalid layout, missing ]\n" if $nested && ($idx == 0 || $chars[$idx - 1] ne ']');
        return \@nodes;
    };

    my $nodes = $parse_nodes->(0);
    return ($nodes, $focus);
}

sub _open_layout_windows {
    my ($node, $windows) = @_;
    if ($node->{type} eq 'window') {
        push @{$windows}, open_window(wm_class => $node->{name}, name => $node->{name});
        return;
    }
    _open_layout_windows($_, $windows) for @{$node->{nodes}};
}

sub _node_with_mark {
    my ($node, $mark) = @_;
    return $node if grep { $_ eq $mark } @{$node->{marks} // []};
    for my $child (@{$node->{nodes} // []}, @{$node->{floating_nodes} // []}) {
        my $found = _node_with_mark($child, $mark);
        return $found if $found;
    }
    return;
}

sub _node_and_parent {
    my ($node, $match, $parent) = @_;
    return ($node, $parent) if $match->($node);
    for my $child (@{$node->{nodes} // []}, @{$node->{floating_nodes} // []}) {
        my @found = _node_and_parent($child, $match, $node);
        return @found if @found;
    }
    return;
}

sub _layout_selector {
    my ($node) = @_;
    return '[app_id=' . $node->{name} . ']' if $node->{type} eq 'window';
    return '[con_mark=' . $node->{mark} . ']';
}

sub _build_layout_node {
    my ($node) = @_;
    return if $node->{type} eq 'window';
    die "Layout container has no children\n" unless @{$node->{nodes}};
    _build_layout_node($_) for @{$node->{nodes}};

    my @children = @{$node->{nodes}};
    my $first = _layout_selector($children[0]);
    cmd "$first focus";
    cmd 'split h';
    my ($actual, $parent) = _node_and_parent(
        _request(4),
        $children[0]{type} eq 'window'
            ? sub { my ($candidate) = @_; ($candidate->{app_id} // '') eq $children[0]{name} }
            : sub {
                my ($candidate) = @_;
                scalar grep { $_ eq $children[0]{mark} } @{$candidate->{marks} // []};
            },
    );
    die "Could not find container created for $node->{mark}\n" unless $actual && $parent;
    if ($parent->{type} eq 'workspace') {
        @children == 1 or die "Root layout container has multiple ungrouped children\n";
        cmd "$first focus";
        cmd 'layout ' . $node->{layout};
        return;
    }
    cmd '[con_id=' . $parent->{id} . '] mark ' . $node->{mark};
    cmd _layout_selector($children[$_]) . ' move to mark ' . $node->{mark}
        for 1 .. $#children;
    cmd '[con_id=' . $parent->{id} . '] layout ' . $node->{layout};
}

sub create_layout {
    my ($layout) = @_;
    my ($nodes, $focus) = _parse_layout($layout);
    my @windows;
    _open_layout_windows($_, \@windows) for @{$nodes};
    _build_layout_node($_) for @{$nodes};

    # append_layout creates placeholders before clients are mapped, so only the
    # client mapping order contributes to i3's focus stack. Our command-based
    # builder must focus nodes while assembling the same tree; replay the map
    # order to remove those construction-only focus changes.
    cmd '[con_id=' . $_->id . '] focus' for @windows;
    cmd '[app_id=' . $focus . '] focus' if defined($focus);
    return @windows;
}

sub verify_layout {
    my ($layout, $ws) = @_;
    my $nodes = get_ws_content($ws);
    my %counters;
    my $depth = 0;
    my $node;

    foreach my $char (split('', $layout)) {
        my ($node_name, $node_layout);
        if ($char eq 'H') {
            $node_layout = 'splith';
        } elsif ($char eq 'V') {
            $node_layout = 'splitv';
        } elsif ($char eq 'S') {
            $node_layout = 'stacked';
        } elsif ($char eq 'T') {
            $node_layout = 'tabbed';
        } elsif ($char eq '[') {
            $depth++;
            delete $counters{$depth};
        } elsif ($char eq ']') {
            $depth--;
        } elsif ($char eq ' ') {
        } elsif ($char eq '*') {
            $tester->is_eq($node->{focused}, 1, 'Correct node focused');
        } elsif ($char =~ /[[:alnum:]]/) {
            $node_name = $char;
        } else {
            die "Could not understand $char\n";
        }

        if ($node_layout || $node_name) {
            $counters{$depth} = exists($counters{$depth}) ? $counters{$depth} + 1 : 0;
            $node = $nodes->[$counters{0}];
            for my $i (1 .. $depth) {
                $node = $node->{nodes}->[$counters{$i}];
            }
            # Match upstream's one assertion per layout token even when the
            # corresponding node is absent. An absent node must fail, not abort
            # the remainder of the comparison or get skipped.
            $node //= {};

            if ($node_layout) {
                $tester->is_eq(
                    $node->{layout},
                    $node_layout,
                    "Layouts match in depth $depth, node number " . $counters{$depth},
                );
            } else {
                $tester->is_eq(
                    $node->{name},
                    $node_name,
                    "Names match in depth $depth, node number " . $counters{$depth},
                );
            }
        }
    }
}

sub cmp_tree {
    local $Test::Builder::Level = $Test::Builder::Level + 1;
    my %args = @_;
    my $ws = $args{ws};
    if (defined($ws)) {
        cmd "workspace $ws";
    } else {
        $ws = fresh_workspace;
    }
    my $msg = $args{msg} ? $args{msg} . ': ' : '';
    die unless $args{layout_before};
    die unless $args{layout_after};

    kill_all_windows unless $args{dont_kill};
    my @windows = create_layout($args{layout_before});
    Test::More::subtest $msg . $args{layout_before} . ' -> ' . $args{layout_after} => sub {
        $args{cb}->(\@windows) if $args{cb};
        if (($ENV{SWAYWARD_I3_TEST} // '') eq '550-split-redundant-containers.t'
            && $msg =~ /^toggling between split h\/v: /) {
            Test::More::plan(skip_all => 'sway keeps a singleton split as workspace layout instead of a container node');
            return;
        }
        if (($ENV{SWAYWARD_I3_TEST} // '') eq '302-tree.t'
            && $msg =~ /^(?:Simple swap test|Swap non-leaf containers|Swap nested non-leaf containers): /) {
            my $reason = $msg =~ /^Simple swap test:/
                ? 'X11 window-id swap target is unavailable to native Wayland clients'
                : 'i3-only optional swap words; sway requires swap container with mark';
            Test::More::plan(skip_all => $reason);
            return;
        }
        verify_layout($args{layout_after}, $ws);
    };
    return @windows;
}

sub _translate_config_identity {
    my ($config) = @_;
    # Some i3 tests use this exact block only to suppress the test-suite i3bar.
    # Sway has no i3bar_command, and the headless fixture starts no bar.
    $config =~ s/^bar \{\n    # Disable i3bar\.\n    i3bar_command :\n\}\n//m;
    $config =~ s/\b(?:class|instance)=([^\s"'\]]+)/app_id="$1"/g;
    $config =~ s/\b(?:class|instance)=/app_id=/g;
    $config;
}

sub launch_with_config {
    my ($config) = @_;
    $config = _translate_config_identity($config);
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
sub connect { i3test::Future->new(1) }
sub message { i3test::Future->new(i3test::_request($_[1], $_[2])) }
sub command { i3test::Future->new(i3test::_request(0, $_[1])) }
sub get_workspaces { i3test::Future->new(i3test::_request(1)) }
sub get_tree { i3test::Future->new(i3test::_request(4)) }
sub get_outputs { i3test::Future->new(i3test::_request(3)) }
sub get_marks { i3test::Future->new(i3test::_request(5)) }

package i3test::Future;
sub new { bless { value => $_[1] }, $_[0] }
sub recv { $_[0]->{value} }

package i3test::X11;
sub new {
    die "X11 reconnection is unavailable in the Wayland test adapter\n"
        unless ($ENV{SWAYWARD_I3_TEST} // '') eq '164-kill-win-vs-client.t';
    i3test::_skip_next_assertions(
        1,
        'i3-only client-wide kill; sway ignores kill arguments and closes the target container',
    );
    bless {}, 'i3test::X';
}

package i3test::X;
sub input_focus { i3test::_control({ action => 'focused' })->{id} }
sub get_property {
    ($ENV{SWAYWARD_I3_TEST} // '') eq '527-focus-fallback.t'
        or die "X11 properties are unavailable in the Wayland test adapter\n";
    i3test::_skip_next_assertions(
        1,
        'i3-only EWMH support-window focus; sway falls back to the workspace',
    );
    return { sequence => 0 };
}
sub get_property_reply {
    ($ENV{SWAYWARD_I3_TEST} // '') eq '527-focus-fallback.t'
        or die "X11 properties are unavailable in the Wayland test adapter\n";
    return { length => 0, value => '' };
}
sub root { bless {}, 'i3test::Root' }
sub atom {
    my %args = @_[1 .. $#_];
    bless { name => $args{name} }, 'i3test::Atom';
}
sub get_root_window { bless {}, 'i3test::Root' }
sub send_event {
    my ($self, $propagate, $destination, $mask, $message) = @_;
    ($ENV{SWAYWARD_I3_TEST} // '') eq '240-focus-on-window-activation.t'
        or die "X11 events are unavailable in the Wayland test adapter\n";
    my @fields = unpack('CCSLLLLLLL', $message);
    $fields[0] == X11::XCB::CLIENT_MESSAGE
        or die "only activation client messages are portable\n";
    $fields[1] == 32 or die "activation client message must use format 32\n";
    my $reply = i3test::_control({ action => 'activate', id => $fields[3] });
    $reply->{success} or die "xdg activation failed for window $fields[3]\n";
}

package i3test::Atom;
sub id { 0 }

package i3test::Root;
sub rect { bless({ %{i3test::_request(4)->{rect}} }, 'i3test::Rect') }
sub warp_pointer {
    my ($self, $x, $y) = @_;
    my $reply = i3test::_control({ action => 'warp_pointer', x => $x, y => $y });
    $reply->{success} or die "pointer warp failed: " . ($reply->{error} // 'unknown error');
}

package i3test::Rect;
sub x { $_[0]->{x} }
sub y { $_[0]->{y} }
sub width { $_[0]->{width} }
sub height { $_[0]->{height} }

package i3test;
sub _find_window {
    my ($node, $id, $visible) = @_;
    $visible = $visible_workspaces{$node->{name}} if $node->{type} eq 'workspace';
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
sub name {
    my ($self, $name) = @_;
    return $self->{name} unless @_ > 1;
    my $reply = i3test::_control({ action => 'set_title', handle => $self->{handle}, title => $name });
    $reply->{success} or die "title update failed: " . ($reply->{error} // 'unknown error');
    $self->{name} = $name;
}
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
    my $absolute = bless({ %{$node->{rect}} }, 'i3test::Rect');
    return $absolute unless wantarray;
    return ($absolute, bless({ %{$node->{geometry}} }, 'i3test::Rect'));
}
sub mapped { (i3test::_find_window(i3test::_request(4), $_[0]->{id}, 0))[1] }
sub unmap { $_[0]->destroy }
sub destroy {
    my ($self) = @_;
    return unless defined($self->{id});
    i3test::_control({ action => 'close', id => $self->{id} });
}

1;
