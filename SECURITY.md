# Security

## Reporting a problem

**Do not open a public issue for a security problem.** Report it privately:

- on GitHub: the **Security** tab of this repository → **Report a vulnerability**, or
- by email: support@trainerday.com, with "OFP security" in the subject.

Say what you found, how to reproduce it, and which part it is in (the core, a table, the Studio, the
JavaScript package). You get an answer within a few days. Please give us time to fix it before you
publish; we credit you in the fix unless you ask us not to.

## What counts

- **A table that gets out of its box**: anything that makes the core read or write outside the packet it
  was handed, run without bound, crash, or use memory without limit. A table is data; if one can do more
  than turn bytes into fields, that is a bug in the core and we want to know.
- **The core** misreading a packet in a way that can be turned against the app that embeds it.
- **The Studio**: its page and its local server (`127.0.0.1` only) being reached or driven by something
  other than the person at the computer, or writing outside the registry and captures folders.
- **The repository itself**: a way to get a change into `main` or into a release without a maintainer's
  review.

A table that simply reads a device wrongly is not a security problem: open an ordinary issue or a pull
request.

## The boundary

This is what the design promises, and what a report should be measured against.

**A table** is data. It can say which packets it matches and how bytes become fields: positions, masks,
conditions, arithmetic. It cannot contain code. It cannot name a file, an address, a program or another
device. It cannot send anything to a device. Its arithmetic is capped in depth and size, so it cannot
make an app hang. The only state it keeps is the last count and time of a running counter (`rate`),
per source. The core that reads tables has no access to the system it runs on: it is built
without the standard library and with unsafe code forbidden.

**A plugin** (code for a device no table can read; designed, not built yet, and never part of this
repository) runs in a sandbox whose whole outside world is: receive a packet from its own device, write
to its own device when the rider has allowed it, set a timer, give out a field, write a log line. No
files, no network, nothing about the rider, the app or the machine.

**Neither boundary is widened**, for any device or any contributor. A change that would give a table or
a plugin a way to reach outside it is refused.

## What is not promised

- **Unreviewed plugins are the rider's own choice.** The project does not host, list or review them. The
  sandbox limits what one can reach; it does not make what it says about a device true.
- **A recording can hold personal data** (a heart rate, a device's name or number). Look at a recording
  before you attach it to an issue or a pull request.
- **Tables marked `draft` or `experimental`** have not been confirmed by enough owners to be trusted for
  anything but trying them.
