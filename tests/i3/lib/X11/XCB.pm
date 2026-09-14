package X11::XCB;
use strict;
use warnings;
use Exporter 'import';

our @EXPORT_OK = qw(
    CLIENT_MESSAGE
    CONFIG_WINDOW_STACK_MODE
    EVENT_MASK_SUBSTRUCTURE_REDIRECT
    GET_PROPERTY_TYPE_ANY
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
sub ICCCM_WM_STATE_NORMAL () { 1 }
sub ICCCM_WM_STATE_WITHDRAWN () { 0 }
sub PROP_MODE_REPLACE () { 0 }
sub STACK_MODE_ABOVE () { 0 }

1;
