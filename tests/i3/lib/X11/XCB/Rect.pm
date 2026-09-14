package X11::XCB::Rect;
use strict;
use warnings;

sub new {
    my ($class, %args) = @_;
    bless \%args, $class;
}
sub x { $_[0]->{x} }
sub y { $_[0]->{y} }
sub width { $_[0]->{width} }
sub height { $_[0]->{height} }

1;
