# Surveys of other projects

What other open-source projects know about fitness devices, written down as **facts**, so OFP can add
devices without copying anyone's code.

## The rule: ideas and facts, never code

QZ (qdomyos-zwift) and GoldenCheetah are GPL projects. Which bytes a device sends and what they mean are
facts, and facts may be learned anywhere. Their code, comments, names and structure are theirs, and none
of it comes into OFP. So every device learned from them goes through two separate steps
(`AGENTS.md`, "Licensing, and the clean-room rule"):

1. **Spec.** One session reads the other project and writes what it learned as a plain description: a
   survey page here, and a `registry/tables/<device>.spec.md` per device. No code, no pseudocode, no
   copied comments, no identifiers, and at the top, where each fact came from.
2. **Table.** A different session, which has **not** seen the other project's code, writes the table from
   the spec, this repository's documentation and, later, a recording of the real device.

The table's `provenance` names its spec. Formulas another project made up (for example a power estimate
from cadence and resistance) are that project's model, not a fact about the device, and are not taken.

## How much we take from other projects

Facts are free to learn, but the people behind QZ and GoldenCheetah did the work of finding them. So OFP
takes **a few, very popular devices** from them, written down as specs the clean-room way, credited in
every spec and table. Everything else comes **from owners**: someone with a device records it in OFP
Studio and sends it (or writes the table), which is original work and the way OFP is meant to grow.
(Decided by the project owner, 2026-11-20.)

## How these were made (2026-11-20)

| Survey | Spec-writing session | Tables written from it |
|---|---|---|
| [`qz-spin-bikes.md`](qz-spin-bikes.md) | a separate assistant session cloned QZ into a scratch folder outside any repository, read it, and wrote only the survey and three spec files | `inspire-ic-bike`, `xcx-bike`, `chrono-bike` (drafts), by a session that read only the spec files |
| [`goldencheetah-devices.md`](goldencheetah-devices.md) | the same way, for GoldenCheetah | see the survey |

Every table written this way starts as `status: draft`: it is decoded in the Studio and left out of what
apps get, until an owner of the device records it and confirms its values.

## What the QZ survey found

Most spin bikes speak a Bluetooth standard (FTMS or Cycling Power), and OFP's standard tables read them
without a table of their own. Four listen-only bikes need one (the Keiser M3i, which OFP already has, and
the three above). About twenty-four need a fixed start command written to them before they send data;
a table cannot do that today (`SECURITY.md`, "The boundary"), so they wait for a decision on fixed
start commands, or for plugins. One needs encryption and is out of scope.
