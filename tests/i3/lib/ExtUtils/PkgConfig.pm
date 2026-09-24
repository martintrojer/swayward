package ExtUtils::PkgConfig;
use strict;
use warnings;

our $VERSION = 0;
our $AUTOLOAD;

sub atleast_version {
    my ($class, $package, $version) = @_;
    # The adapter implements XKB group changes in-process; it does not use xcb-xkb.
    return 1 if $package eq 'xcb-xkb';
    system($ENV{PKG_CONFIG} // 'pkg-config', "--atleast-version=$version", $package);
    die "cannot run pkg-config: $!\n" if $? == -1;
    return $? == 0;
}

sub AUTOLOAD {
    my $method = $AUTOLOAD =~ s/.*:://r;
    die "ExtUtils::PkgConfig::$method is unavailable in the i3 test adapter\n";
}

sub DESTROY {}

1;
