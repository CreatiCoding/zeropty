#!/usr/bin/perl
# Replays an zeropty .trace ([u64 ms][u32 len][bytes]...) to stdout at its recorded pace, forever.
# usage: replay_trace.pl <trace> [phase 0..1]
# Starts at a point in the recording (the part before it is written at once), so many replays are out of
# phase, as many real sessions are: at `phase` of the way through when given (benchmarks pass one per
# session, so runs repeat exactly), at a random point otherwise.
use strict; use Time::HiRes qw(time sleep);
open(my $f, '<:raw', $ARGV[0]) or die; local $/; my $d = <$f>; close $f;
my @frames; my $i = 0;
while ($i + 12 <= length $d) {
  my ($lo, $hi, $len) = unpack('VVV', substr($d, $i, 12));
  push @frames, [($lo + $hi * 4294967296) / 1000, substr($d, $i + 12, $len)]; $i += 12 + $len;
}
$| = 1;
my $skip = (defined $ARGV[1] ? $ARGV[1] : rand()) * $frames[-1][0];
my $first = 1;
while (1) {
  my $t0 = time - ($first ? $skip : 0);
  for my $fr (@frames) { my $w = $t0 + $fr->[0] - time; sleep($w) if $w > 0; print $fr->[1]; }
  $first = 0;
  sleep(1);
}
