package i3test::Util;
use strict;
use warnings;
use Exporter qw(import);

our @EXPORT_OK = qw(slurp);

sub slurp {
    my ($file) = @_;
    local $/;
    open my $fh, '<', $file or die "could not open $file: $!";
    return <$fh>;
}

1;
