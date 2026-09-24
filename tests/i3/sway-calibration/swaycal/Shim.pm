package swaycal::Shim;
# Preloaded with -Mswaycal::Shim before an unmodified i3 t/*.t file runs.
#
# It loads upstream i3test.pm and then replaces exactly one function:
# sync_with_i3. Everything else -- open_window, cmd, get_ws, cmp_tree, the
# X11 connection, the assertions -- stays upstream.
#
# WHY sync_with_i3 MUST BE REPLACED
#
# i3's version sends an I3_SYNC ClientMessage to the X root window and blocks
# until i3 echoes it back (i3/testcases/lib/i3test.pm.in:753-804;
# i3/src/handlers.c:813-815). Because i3 replies only after draining every
# earlier event, the reply is a true barrier.
#
# Sway implements no such protocol: `grep -rn "I3_SYNC\|i3_sync" sway/` at
# 88869399 returns nothing, and the 2018 attempt to add one (swaywm/sway#3318)
# was dropped. Left unreplaced, every sync_with_i3 call blocks for the full
# 4-second wait_for_event timeout and the file times out rather than failing.
#
# WHAT THIS BARRIER ACTUALLY GUARANTEES, AND WHAT IT DOES NOT
#
#   1. An X11 round trip (GetInputFocus reply). Our own X requests have
#      reached the X server, so Xwayland has them.
#   2. An IPC round trip (SEND_TICK, sway/ipc-server.c:671-673,741-781).
#      Sway has drained its IPC socket up to this point, so every earlier
#      `cmd` has been executed.
#
# It does NOT guarantee that events already queued on sway's XWM fd have been
# processed, because wl_event_loop does not order work across file
# descriptors. Step 3 is therefore a bounded settle poll: repeat the tick
# round trip until GET_TREE is byte-identical twice in a row, up to a cap.
# That is weaker than i3's barrier and it is the main measurement limit of
# this runner. A sway-side failure in a window-lifecycle test should be
# re-run before it is believed.

use strict;
use warnings;
use v5.10;

use IO::Socket::UNIX;
use List::Util ();
use AnyEvent::I3 ();

require i3test;

sub _sock {
    # Re-read every time: launch_with_config starts a new sway with a new path.
    return i3test::get_socket_path(0);
}

sub _ipc {
    my ($type, $payload) = @_;
    $payload //= '';
    my $path = _sock();
    my $cl = IO::Socket::UNIX->new(Peer => $path) or return undef;
    print $cl 'i3-ipc' . pack('LL', length($payload), $type) . $payload;
    $cl->flush;
    my $hdr;
    read($cl, $hdr, 14) == 14 or do { close($cl); return undef };
    my ($len) = unpack('x6LL', $hdr);
    my $body = '';
    while (length($body) < $len) {
        my $chunk;
        my $n = read($cl, $chunk, $len - length($body));
        last if !defined($n) || $n == 0;
        $body .= $chunk;
    }
    close($cl);
    return $body;
}

my $nonce = 0;

sub _tick {
    return defined _ipc(10, 'swaycal-' . $nonce++);   # IPC_SEND_TICK
}

sub _tree {
    return _ipc(4, '');                                # IPC_GET_TREE
}

sub sway_sync {
    # Accept and ignore i3's no_cache/window_id options. They select details
    # of I3_SYNC, which sway does not implement.

    # 1. X11 round trip.
    eval {
        my $x = $i3test::x;
        $x->get_input_focus_reply($x->get_input_focus()->{sequence}) if $x;
        1;
    };

    # 2 + 3. Tick round trip, then settle until the tree stops changing.
    my $prev;
    for my $i (1 .. 20) {
        _tick();
        my $now = _tree();
        return 1 if defined($prev) && defined($now) && $prev eq $now;
        $prev = $now;
        select(undef, undef, undef, 0.02);
    }
    return 0;
}

{
    no warnings 'redefine';
    no strict 'refs';
    *i3test::sync_with_i3 = \&sway_sync;
}

# ---------------------------------------------------------------------------
# OPTIONAL second pass: the output "content" node.
#
# Enabled only when SWAY_CAL_CONTENT_SHIM=1. The default run does NOT have it,
# and the default run is the primary measurement.
#
# i3 puts a CT_CON "content" node between an output and its workspaces, and
# five i3test.pm helpers navigate through it with
#   first { $_->{type} eq 'con' } @{$output->{nodes}}
# (i3test.pm.in:406,430,474,519,723). Sway serializes each workspace as a
# direct child of the output with no intervening node
# (sway/sway/ipc-json.c:869-874). The helper therefore gets undef and dies
# before the file's first assertion. That kills 140 of 242 files at i3test.pm
# line 407, in the harness, not in an assertion.
#
# This replaces those five helpers with versions that read
# $output->{nodes} directly. It changes ONLY workspace lookup. Every
# assertion, expected value and comparison stays upstream, and no .t file is
# touched.
#
# It is still a real weakening, which is why it is opt-in and reported
# separately: a test that asserts something ABOUT the content node would now
# be answered from a node that does not exist in sway. Treat any result from
# this pass as conditional on that.
if ($ENV{SWAY_CAL_CONTENT_SHIM}) {
    no warnings 'redefine';
    no strict 'refs';

    my $workspaces_of = sub {
        my $tree = AnyEvent::I3::i3(i3test::get_socket_path())->get_tree->recv;
        my @ws;
        for my $output (@{$tree->{nodes}}) {
            next if ($output->{name} // '') eq '__i3';
            push @ws, grep { ($_->{type} // '') eq 'workspace' } @{$output->{nodes}};
        }
        return @ws;
    };

    *i3test::get_workspace_names = sub {
        [ map { $_->{name} } $workspaces_of->() ]
    };

    *i3test::get_ws = sub {
        my ($name) = @_;
        return List::Util::first { $_->{name} eq $name } $workspaces_of->();
    };

    *i3test::get_ws_content = sub {
        my ($name) = @_;
        my $con = i3test::get_ws($name);
        return wantarray ? ($con->{nodes}, $con->{focus}) : $con->{nodes};
    };

    *i3test::get_output_for_workspace = sub {
        my ($ws_name) = @_;
        my $tree = AnyEvent::I3::i3(i3test::get_socket_path())->get_tree->recv;
        for my $output (@{$tree->{nodes}}) {
            next if ($output->{name} // '') eq '__i3';
            for my $ws (@{$output->{nodes}}) {
                return $output->{name} if ($ws->{name} // '') eq $ws_name;
            }
        }
        return '';
    };

    *i3test::focused_ws = sub {
        my $tree = AnyEvent::I3::i3(i3test::get_socket_path())->get_tree->recv;
        my ($output) = grep {
            ($_->{type} // '') eq 'output' && ($_->{name} // '') ne '__i3'
                && @{$_->{focus} // []}
        } @{$tree->{nodes}};
        return undef unless $output;
        my $focused = $output->{focus}->[0];
        my $ws = List::Util::first { $_->{id} == $focused } @{$output->{nodes}};
        return $ws ? $ws->{name} : undef;
    };
}

1;
