package ExtUtils::PkgConfig;
use strict;
use warnings;

our $VERSION = 0;
our $AUTOLOAD;

sub atleast_version {
    my ($class, $package, $version) = @_;
    system('pkg-config', "--atleast-version=$version", $package);
    die "cannot run pkg-config: $!\n" if $? == -1;
    return $? == 0;
}

sub AUTOLOAD {
    my $method = $AUTOLOAD =~ s/.*:://r;
    die "ExtUtils::PkgConfig::$method is unavailable in the i3 test adapter\n";
}

sub DESTROY {}

1;
