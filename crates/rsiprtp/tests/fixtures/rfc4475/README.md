# RFC 4475 fixtures

[RFC 4475](https://datatracker.ietf.org/doc/html/rfc4475) ("Session
Initiation Protocol (SIP) Torture Test Messages") defines a corpus of
intentionally-stressful SIP messages designed to exercise parser
corners. These fixtures are byte-exact decodes of RFC 4475 Appendix
A.1 (the encoded reference messages). They are protected by a
`*.sip -text` rule in the repo-root `.gitattributes` so the bytes
round-trip git verbatim across platforms — the SHA-256 column below
is reproducible on any host.

Each `.sip` file is the canonical wire form: CRLF line endings, a
`\r\n\r\n` body separator, and (where applicable) a body that may
contain bare octets including `0x0a` inside binary attachments. Do
not edit these files; they are the artifact under test.

## §3.1.1 Valid Messages — catalog

All 13 §3.1.1 valid-message sub-sections have byte-exact fixtures (12 accepted; `intmeth` documented-rejection by closed-set Method policy per §3.1.1.2 prose).

| File | Description | RFC § | SHA-256 |
|---|---|---|---|
| `wsinv.sip` | Short tortuous INVITE: quoted display names with embedded SP / quoted-pairs, parameter-name-only forms, exotic interior whitespace, and folded headers. (Pinned divergence — rsip 0.4 rejects the whole message; see `diff_rfc4475_wsinv_rsip_rejects`.) | §3.1.1.1 | `d6f6cdee99cd0a1f8e3d9a0fdd67c877700e19ab86d8186dada06f61260e3e61` |
| `intmeth.sip` | Wide range of valid characters in the method token (every byte legal under RFC 3261 §25.1 `token`, including `! * + . ' ~ %`). Both parsers reject — ours by closed-set `Method` enum policy (RFC 3261 §7.1), rsip 0.4 by tokenizer narrowness; see `diff_rfc4475_intmeth_both_reject`. | §3.1.1.2 | `b957a8292c1c0a9851fe94705b90f94320738f43aacde64998e401f742ef688d` |
| `esc01.sip` | Valid `%HH` escapes throughout user, contact, and URI parameter portions; the canonical bytes also line-fold the `Contact:` header. (Pinned divergence — rsip 0.4 rejects the SP-led continuation; see `diff_rfc4475_esc01_rsip_rejects_folding`.) | §3.1.1.3 | `be3a316d4ce5f69cad474646191e390dd4b50fc8371e44b5197fe1237781973e` |
| `escnull.sip` | Escaped NUL bytes (`%00`) in the user portion of `To:`, `From:`, and `Contact:` URIs. | §3.1.1.4 | `263fe2ecd12a5ccbb6c67b3174e9211d11da9b600e7d69666a86d2ee42873e90` |
| `esc02.sip` | `%` characters in header *values* that are not `%HH` escape sequences. | §3.1.1.5 | `9a9c59449f00327b02071feafc8e5c2a261c98c818044167894cc17809a12199` |
| `lwsdisp.sip` | Display name and `<addr-spec>` with no LWS between them (e.g. `"caller"<sip:caller@…>`). | §3.1.1.6 | `d1b36b6316c12824f6f04bf6b1fd1f021cade9b74871693b9c3474cd9fa7f4e6` |
| `longreq.sip` | Long header values — exercises the value-length path within our 8192-byte defense-in-depth cap. The canonical bytes also carry HCOLON-with-interior-whitespace forms in the `Via:` stack. (Pinned divergence — rsip 0.4 rejects the HCOLON-whitespace forms; see `diff_rfc4475_longreq_rsip_rejects_hcolon_whitespace`.) | §3.1.1.7 | `185739292bc676760e5b1cca77c87705aa570240171c506ad76f8c35af0ecdaa` |
| `dblreq.sip` | `Content-Length: 0` request with extra octets after the body (a complete second request). The extra bytes must be ignored. (Pinned divergence — rsip 0.4 captures the trailing bytes as the body; see `diff_rfc4475_dblreq_rsip_keeps_trailing`.) | §3.1.1.8 | `a6be48426565c2a705389c4ce2ea17e57b891cb13f80490a2cb9392c797b46ef` |
| `semiuri.sip` | Semicolons in URI user part (`sip:user;par=u%40example.net@example.com`). (Pinned divergence — rsip 0.4 rejects; see `diff_rfc4475_semiuri_rsip_rejects`. M6 also fixed a bug in our URI parser where `;` before `@` was treated as the params boundary.) | §3.1.1.9 | `87bbaed6b4b354dbd3c5c7a20e66686303c63e43a0aa19b89de1ac75231611a2` |
| `transports.sip` | Five `Via:` lines covering, in order, UDP, SCTP, TLS, `UNKNOWN`, and TCP transports. (Pinned divergence — rsip 0.4's typed-Via rejects unknown transport tokens; see `diff_rfc4475_transports_rsip_rejects_unknown_transport`.) | §3.1.1.10 | `33b4553a3b9418121b099ff07882535151edd6a1e81898c63a916aad2cbf90bc` |
| `mpart01.sip` | Multipart MIME `MESSAGE` request — first part `text/plain`, second part `application/octet-stream` carrying a binary (DER-encoded) attachment. The body contains three bare LF (`0x0a`) octets that are *not* line terminators; tier-1 framing finds `\r\n\r\n` first and the body rides through verbatim. | §3.1.1.11 | `92147c8e24997bac59629e1b163767697d0d6d499af87f89e325d69968bec3ba` |
| `unreason.sip` | 200 response whose Reason-Phrase carries non-ASCII (UTF-8 Cyrillic) bytes — exercises RFC 3261 §25.1 `Reason-Phrase` UTF8-NONASCII / UTF8-CONT grammar. | §3.1.1.12 | `4f939d6ebf4817eea70f011d3209e83732872ffc20ffd304b244084dc392f4d7` |
| `noreason.sip` | `SIP/2.0 100 \r\n` status line — Reason-Phrase is empty (just the trailing SP after the status code). RFC 3261 §25.1 `Reason-Phrase = *(...)` admits zero length. | §3.1.1.13 | `07e11e470a69fffa3674de805e5165d1351a9439a91045ee70c9570ea24ab4fe` |

The hashes above are reproducible on any host with
`Get-FileHash -Algorithm SHA256 *.sip` (PowerShell) or
`sha256sum *.sip` (POSIX), provided the `*.sip -text` rule in the
repo-root `.gitattributes` is active for the working tree.

## §4 Invalid Messages — `rfc4475_invalid/`

Lives in a sibling directory (`crates/rsiprtp/tests/fixtures/rfc4475_invalid/`)
to make the rejection-expectation explicit. Each fixture is asserted
to be rejected by **both** rsip 0.4 and our parser via
`assert_both_reject` in `parser_diff.rs`.

| File | Description | RFC § category |
|---|---|---|
| `badaspec_no_version.sip` | Request line missing the `SIP/2.0` SIP-Version token entirely. | §4 (badaspec) |
| `badaspec_garbage_start.sip` | Start line that is neither a valid request line nor a valid status line. | §4 (badaspec) |

The §4 corpus is intentionally narrow here — only the `badaspec_*`
shapes that exercise tier-1 framing rejection. The full RFC 4475 §4
set (`ncl`, `scalar*`, `lwsruri`, `badinv01`, `regbadct`, …) raises
per-fixture questions (do both parsers reject for the right reason?
does our parser reject at framing or only at typed-form?) that
deserve their own corpus expansion. Out of scope for this milestone.

The RFC 4475 §4 `ncl` test (negative `Content-Length`) was considered
and dropped: both parsers store header values as strings and only
validate the digits when bounding the body, which is a typed-form /
body-extraction concern rather than a tier-1 framing concern. The §4
ncl test really exercises tier-2 logic that this harness does not
cover.

## Pinned divergences from this milestone (M6 + byte-perfect upgrade)

| Test | Direction | Spec citation |
|---|---|---|
| `diff_rfc4475_wsinv_rsip_rejects` | rsip rejects, ours accepts | RFC 3261 §7.3.1 (line folding) + §25.1 (LWS in HCOLON / SEMI) |
| `diff_rfc4475_esc01_rsip_rejects_folding` | rsip rejects, ours accepts | RFC 3261 §7.3.1 (line folding) — canonical §A.1 bytes fold the `Contact:` header |
| `diff_rfc4475_longreq_rsip_rejects_hcolon_whitespace` | rsip rejects, ours accepts | RFC 3261 §25.1 `HCOLON = *( SP / HTAB ) ":" SWS` — canonical §A.1 Via stack uses interior whitespace around `:` |
| `diff_rfc4475_intmeth_both_reject` | both reject (ours by §7.1 closed-set policy; rsip by tokenizer narrowness) | RFC 3261 §7.1 method-token grammar; RFC 4475 §3.1.1.2 prose explicitly allows "501 Not Implemented" |
| `diff_rfc4475_dblreq_rsip_keeps_trailing` | both accept; rsip captures trailing octets as body, ours truncates per Content-Length | RFC 3261 §18.3 / RFC 4475 §3.1.1.8 |
| `diff_rfc4475_semiuri_rsip_rejects` | rsip rejects, ours accepts | RFC 3261 §25.1 `user-unreserved` includes `;`; §19.1.1 grammar requires `@` to bound userinfo before params |
| `diff_rfc4475_transports_rsip_rejects_unknown_transport` | rsip's typed-Via rejects, ours accepts | RFC 3261 §20.42 `transport-param` `other-transport = token` |

Combined with the pre-M6 pins (`diff_handcrafted_invite_folded_subject`,
`typed_from_quoted_param_value_rsip_rejects_broadly`,
`typed_from_quoted_param_value_with_semicolon_rsip_rejects`,
`typed_via_ipv6_rsip_rejects`), the pre-fuzz hardening pin
(`typed_contact_wildcard_with_params_rsip_misclassifies` — RFC 3261
§10.2.2 `Contact: *;expires=0` is the canonical REGISTER unbinding
shape; our parser produces a typed `Wildcard { params }`, rsip 0.4
misclassifies the `*` as a `Domain` host of an addr-spec), the M11
fuzz finding #10
(`typed_status_line_sip1_x_version_rsip_accepts_we_reject` — rsip
accepts `SIP/1.x` and other arbitrary `SIP/N.M` versions; RFC 3261
§7.1 mandates exactly `SIP/2.0`), the M11 fuzz finding #11
(`body_leading_crlf_rsip_strips_we_preserve` — rsip silently strips a
leading `\r\n` from the body when the wire bytes carry a third CRLF
immediately after the headers/body separator; RFC 3261 §7.5 says the
body is exactly the bytes after the separator, so the third CRLF
*belongs to* the body), the M11 fuzz finding #13
(`header_missing_colon_rsip_accepts_we_reject` — rsip silently
absorbs a bare LF, without preceding CR, into the status-line
Reason-Phrase, consuming the next line's bytes; RFC 3261 §7.2 BNF
mandates CRLF as the line terminator and excludes LF from the
Reason-Phrase character set. The "missing ':'" error from our parser
is the visible proxy for this rsip-side issue.), the M11 fuzz
finding #14
(`header_section_contains_nul_rsip_rejects_we_accept` — rsip 0.4's
nom-based tokenizer rejects a NUL byte (`0x00`) in a header value
with a `Tokenizer error`; RFC 3261 §7.3 does not strictly forbid NUL
in header values and §25.1 OCTET grammar admits any byte. Our parser
accepts per the M2-A pinned permissive policy
(`test_header_with_embedded_nul_pinned_accepted`)), and the M11 fuzz
finding #6
(`body_starts_with_header_like_line_rsip_misinterprets` — a bare LF
in the start-line region (before the first `\r\n`) trips two mutually
amplifying non-RFC behaviors: rsip 0.4 absorbs the bare LF into the
reason phrase via `take_until("\r\n")` (same family as #13), while our
`find_separator` LFLF fallback splits at the bare LFLF. Both parsers
accept the same status code but disagree on framing — rsip parses
some bytes as headers that we surface as body. RFC 3261 §7.1/§7.2
mandate CRLF as the line terminator; both parsers are non-strict but
the kind / status agree. The oracle's `(Ok, Ok)` arm carries a
`has_bare_lf_in_start_line` predicate that catches this whole class
without enumerating wire shapes), the running rsip 0.4
spec-deficiency count is **14 active distinct types** (the
byte-perfect upgrade did not add new types — the new `esc01` and
`longreq` pins are in the same families as the existing `wsinv` pin:
folding (§7.3.1) and HCOLON-whitespace (§25.1)). All are retargeted
to direct on-our-parser assertions when rsip is dropped from runtime
deps at M10.

The `(Err, Ok)` arm of the fuzz oracle (rsip rejects, we accept) now
uses a **principled heuristic** rather than per-error-string skips:
"non-printable byte in the header section (anything outside
0x09/0x0A/0x0D/0x20-0x7E) AND rsip Tokenizer-class error" is treated
as documented asymmetry. This catches the broader class of "rsip
tokenizer narrower than our parser" findings — including future
high-bit, lone-CR, and other-control-byte mutations the fuzzer may
discover — without us having to enumerate visible rsip error-message
wrappings. Findings #12, #13, #14 are the canonical instances; the
heuristic prevents libfuzzer from rediscovering each variant during
the campaign. See `parser_diff_oracle::assert_equivalent`.

M11 fuzz finding #12 (status line missing SP after status code) was
**closed at the framing layer** rather than pinned: per RFC 3261 §7.2
BNF the SP between Status-Code and Reason-Phrase is mandatory, so
`parse_status_line` was tightened to match (see
`test_status_line_missing_sp_after_code_rejects`). The previous
asymmetry pin and oracle skip were retired in favor of the symmetric
both-reject test `status_line_missing_sp_after_code_both_reject` in
`parser_diff.rs`.

## Parser bug fixed in this milestone

`crates/rsiprtp/src/sip/uri.rs` — the URI parser's parameter-boundary
detection treated the *first* `;` as the user/host vs params boundary,
even when that `;` lay inside the userinfo (i.e., before the `@`). RFC
3261 §19.1.1 requires `@` to terminate userinfo before parameter
parsing begins. Surfaced by `wsinv.sip` and `semiuri.sip`; fixed by
restricting the `;` search to the substring after the `@` (or the
whole rest if there is no `@`).

## Regenerating from RFC 4475

The §A.1 corpus is published as a single base64-encoded gzip-compressed
tar archive embedded in the RFC text between
`-- BEGIN MESSAGE ARCHIVE --` and `-- END MESSAGE ARCHIVE --` markers.
**Trap:** the RFC text contains *two* such marker pairs — the first
is inside the Perl decoder source listing (illustrative, not data),
the second is the real archive. Always take the **second** pair.

The decode procedure (rerun this if the upstream RFC publishes
errata or if you ever need to verify the bytes from scratch):

```sh
# 1. Fetch the RFC 4475 plain-text form
curl -sSL https://www.rfc-editor.org/rfc/rfc4475.txt -o rfc4475.txt

# 2. Extract the second BEGIN/END MESSAGE ARCHIVE block
#    (the first one is inside the Perl decoder source listing).
#    Strip the page-header / RFC line-numbering noise as you go.

# 3. base64 -d | gunzip | tar x
#    Yields per-message files: wsinv, intmeth, esc01, escnull, esc02,
#    lwsdisp, longreq, dblreq, semiuri, transports, mpart01, unreason,
#    noreason, plus the §4 invalid set (ncl, scalar02, scalarlg,
#    lwsruri, badinv01, regbadct, …).

# 4. Compare SHA-256 of each tar member against the table above.
```

The 13 `.sip` files in this directory are the verbatim tar-member
bytes for the 13 §3.1.1 valid messages. No transformation; no
`include_bytes!` reformatting; the bytes on disk are the bytes the
parser sees.

## External corroboration

The byte-perfect claim is independently verifiable.
[`github.com/josephfrazier/rfc4475`](https://github.com/josephfrazier/rfc4475)
is an independent decode of RFC 4475 §A.1 published as plain
filesystem artifacts. **All 13 fixtures hash-match the corresponding
`<name>.dat` files in that repository byte-for-byte** (verified by
fetching `https://raw.githubusercontent.com/josephfrazier/rfc4475/master/<name>.dat`
for each `<name>` in the catalog table above and computing
SHA-256). Two independent decodes producing the same hashes for
every member of the §3.1.1 corpus is stronger evidence than either
one in isolation.

## Errata

The RFC Editor errata database reports **"Found 0 records"** for
RFC 4475 as of 2026-05-06, confirmed via
<https://www.rfc-editor.org/errata_search.php?rfc=4475> and
<https://www.rfc-editor.org/errata/rfc4475>. No errata affect the
§A.1 reference messages, so the SHA-256 column above is the
authoritative byte set. If errata are filed in the future, this
section should be re-checked and (if necessary) the fixture bytes
regenerated.
