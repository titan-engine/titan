# Platformer shared level data

This package owns the shared level data for Titan's sample platformer game. It
contains a validated Rust model and text parser/writer for `.platformer` level
files. The package is game-specific; Titan's reusable libraries and launcher do
not know about platforms or spawn points.

The sample game is not playable yet. This library does not provide a game
binary, filesystem access, an editor, or level editing operations.

## Format

A document consists of a single header followed by records. The first nonblank
line must be the header, and blank lines are ignored. Both LF and CRLF line
endings are accepted. Fields are separated by whitespace, and every record
must have exactly the fields shown here:

```text
platformer-level 1
next-id <nonzero unsigned 64-bit integer>
spawn <finite f32 x> <finite f32 y>
platform <nonzero unique u64 id> <finite f32 x> <finite f32 y> <positive finite f32 width> <positive finite f32 height>
```

Exactly one `next-id` and one `spawn` record are required. Platform, spawn, and
`next-id` records may appear in any order after the header. A platform's
position is its lower-left corner; positive X points right and positive Y points
up. Platform right and top bounds (`x + width` and `y + height`) must remain
finite. The `next-id` value must be greater than every platform ID and must be
nonzero even when there are no platforms. Unknown versions, records, duplicate required
records, duplicate platform IDs, missing or extra fields, malformed numbers,
and non-finite or otherwise invalid values are rejected.

The Rust `Level` type implements `FromStr` and `Display`. Parsed platforms are
stored and written in ascending ID order. Writing uses LF line endings and the
canonical record order shown below; formatting and parsing preserve finite
`f32` values.

## Valid example

```text
platformer-level 1
next-id 4
spawn 64 64
platform 1 0 0 400 32
platform 2 448 64 128 24
platform 3 640 128 160 24
```
