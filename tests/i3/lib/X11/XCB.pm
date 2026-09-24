package X11::XCB;
use strict;
use warnings;
use Exporter 'import';

our @EXPORT_OK = qw(
    CLIENT_MESSAGE
    CONFIG_WINDOW_STACK_MODE
    EVENT_MASK_SUBSTRUCTURE_REDIRECT
    GET_PROPERTY_TYPE_ANY
    eq_hash
    ICCCM_WM_STATE_NORMAL
    ICCCM_WM_STATE_WITHDRAWN
    PROP_MODE_REPLACE
    STACK_MODE_ABOVE
);
our %EXPORT_TAGS = (all => \@EXPORT_OK);
our $VERSION = 0;

# Load-only compatibility for upstream tests. The values are never interpreted
# by swayward; any X11 operation that consumes them remains deliberately absent
# and therefore dies at the call site.
sub CLIENT_MESSAGE () { 33 }
sub CONFIG_WINDOW_STACK_MODE () { 1 << 6 }
sub EVENT_MASK_SUBSTRUCTURE_REDIRECT () { 1 << 20 }
sub GET_PROPERTY_TYPE_ANY () { 0 }
sub eq_hash {
    my ($left, $right) = @_;
    return $left->{x} == $right->{x}
        && $left->{y} == $right->{y}
        && $left->{width} == $right->{width}
        && $left->{height} == $right->{height};
}
sub ICCCM_WM_STATE_NORMAL () { 1 }
sub ICCCM_WM_STATE_WITHDRAWN () { 0 }
sub PROP_MODE_REPLACE () { 0 }
sub STACK_MODE_ABOVE () { 0 }

package X11::XCB::Connection;
sub new { die "X11::XCB::Connection is unavailable in the Wayland test adapter\n" }

package X11::XCB::Sizehints::Aspect;
sub new { die "X11::XCB::Sizehints::Aspect is unavailable in the Wayland test adapter\n" }

1;
