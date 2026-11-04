# Sagar content server API v1

For endpoint request/response schemas, curl examples and client integration, see
the [detailed REST API guide](../docs/reset.md).

Content API and Sagar settings controls for a future Sagarwar client. In the
library, open Settings → Content server to Start/Stop and check connection
addresses. No Sagarwar code is included. The server is OFF at launch; it does not persist
credentials or automatically restart. It shares the entire local Sagar library
while running. Original EPUB bytes and saved translations remain separate.

## Local lifecycle (Tauri commands)

- `start_content_server({config: {port: 8080, username: "", password: ""}})`
- `content_server_status()`
- `stop_content_server()`

The typed bridge is `src/lib/tauri/contentServer.ts`. Port 0 chooses an available
port. Both credentials default to empty (authentication disabled); otherwise
both are required. Username must not contain a colon. Credentials remain in
memory and are never returned in status. Start returns
`{running: true, port: 8080, authenticationRequired: false, urls: ["http://192.168.1.20:8080"]}`.
The settings screen defaults to port 8080; discovered LAN IPv4 addresses are
shown for the client connection, with a fallback explanation if none are found.
Stop is idempotent. Stop before changing the port or credentials. App exit
ends the listener. Bind failures and duplicate starts return structured errors.

The listener binds IPv4 `0.0.0.0`; clients use the desktop's LAN IPv4 address.
This is plain HTTP for a trusted local network, without TLS, discovery, pairing,
or router configuration. HTTP Basic authentication does not encrypt passwords
or book content. Do not expose the port to the public internet. An empty login
allows any native client that can reach the port to download books and fill
missing translations. Browser-origin requests are rejected; no CORS is enabled.
Requests are limited to eight concurrent handlers with a 30-second handler/body
read deadline (EPUB response bodies stream separately). Slow/overloaded requests
return 408/503. All responses use `Cache-Control: no-store`. No request/content logging is added.

## Remote endpoints

Every endpoint requires `Authorization: Basic <base64(username:password)>` when
credentials are configured. Missing/wrong credentials return 401 with
`WWW-Authenticate`. API paths are case-sensitive.

| Method | Path | Result |
| --- | --- | --- |
| GET | `/api/v1` | Server name, version, translation policy and HTML format |
| GET | `/api/v1/books` | Array of books with `id`, `sourceSha256`, `title`, `author`, `coverUrl`, `sourceLanguage`, `targetLanguage` |
| GET | `/api/v1/books/{bookId}/cover` | Cover image bytes with image content type; 404 when unavailable |
| GET | `/api/v1/books/{bookId}/epub` | Unmodified source snapshot, `application/epub+zip` |
| GET | `/api/v1/books/{bookId}/chapters` | Spine-ordered chapter array, including saved HTML |
| GET | `/api/v1/books/{bookId}/chapters/translated` | `{chapterIds: [...]}` for saved translation HTML, in spine order |
| PUT | `/api/v1/books/{bookId}/chapters/{chapterId}/translation` | Fill an empty chapter or skip existing content |

Each book includes an origin-relative `coverUrl`, for example
`/api/v1/books/<bookId>/cover`. Resolve it against the server origin and send
the same authentication credentials. The route is advertised even when no
cover is available; show a placeholder on 404. The cover uses Sagar's custom
Book Settings cover first, then the original EPUB cover, using the existing
library cover loader and resource limits. Supported types are PNG, JPEG, GIF,
WebP, AVIF, BMP and SVG; unsupported media types return 404. SVG responses
have a restrictive sandbox CSP; clients must still load covers as images,
never as privileged documents. Covers are fetched separately so the book list
does not embed image bytes or scan every EPUB. Updating a custom cover does
not change the original EPUB download or its identity hash.

Book IDs and chapter IDs are server-local identifiers, not cross-app identity.
Chapter entries contain `id`, `href`, `title`, `spineIndex`, and `document`:
`{html: string|null, status: string, revision: number, authored: boolean}`.
HTML is a chapter **body fragment**, not a complete XHTML document. Relative
resource URLs resolve against `href` in the original EPUB. Preserve meaningful
markup, IDs, links and whitespace. Clients must validate/contain received HTML;
never execute it in a privileged web view. Downloaded HTML is not rendered by
this server. Existing legacy block translations are surfaced through the same
chapter document adapter as the editor.

The lightweight `/chapters/translated` route returns server-local chapter IDs
with saved nonempty HTML, including drafts and legacy fallback translations.
It returns `{chapterIds: []}` for a book with none, or 404 for a missing book.
It does not create chapter document rows. Saved chapter documents take
precedence over legacy blocks. Contentless Keep original choices are not in
this list; still consult chapter statuses and honor PUT skip results. IDs need
resource mapping before comparison with another app's chapter IDs. Fetch the
full chapters endpoint when HTML or revision information is needed. Only the
correctly spelled `translated` route is supported.

PUT body (maximum 8 MiB JSON):

```json
{
  "sourceSha256": "<SHA-256 of original EPUB bytes>",
  "targetLanguage": "my",
  "href": "EPUB/Text/chapter01.xhtml",
  "html": "<p>မြန်မာစာ</p>",
  "expectedRevision": 0
}
```

Unknown JSON fields are rejected. Exact original hash, language and resource
path must match. Titles/authors are display hints, not identity checks. Compare
language codes consistently before syncing. Different editions or regrouped
chapters require mapping in a later compatibility task; do not match by title
or chapter number alone. Sagarwar translations spanning multiple resources must
be split into matching per-resource body fragments before upload.

Results:

- `{outcome: "imported", revision: 1, status: "draft"}`
- `{outcome: "skippedExisting"}` — no write or revision change

Any nonempty saved chapter HTML, legacy translated block, or explicit Keep
original choice protects the destination. Existing content is never compared
for quality or overwritten. Imported HTML follows the editor's active-content
validation and revision service and is always a draft. Empty translations are
rejected. Unsupported passive HTML can be retained just as in the current editor;
import success does not assert Rich-editor or EPUB export conformance.
Identity checks, emptiness check and save run under one SQLite write transaction.
Concurrent imports and repeated successful uploads therefore import once.
A stale revision on an empty destination returns 409. Refresh and reconcile;
never force overwrite. Other statuses: 404 missing book/chapter, 422 invalid
translation, 413 oversized JSON, 400 malformed JSON, 415 wrong content type,
503 temporarily busy storage. Domain errors use `{code,message}`; framework
routing/JSON errors may return plain text. Clients must handle status first.

## Sagarwar integration sequence (later)

1. Connect to the desktop URL, optionally supplying Basic credentials.
2. List books; match by original EPUB SHA-256 plus target language.
3. For Add Book, download the original EPUB, verify its hash, import locally,
   then fetch chapter translations. Never use a translated EPUB's hash as the
   original book identity.
4. Flush local pending edits before sync. Fetch the server's chapter inventory.
5. Local empty/server translated: validate HTML and save locally only if still
   empty. Server empty/local translated: PUT with the server's revision.
6. Both translated: skip, including drafts. Both empty: skip.
7. Show imported/uploaded/skipped/failed counts. Retry failed transfers safely.

There is no distributed all-or-nothing transaction: downloads and per-chapter
uploads can partially succeed. Reconciliation on retry fills the remainder.
Unsaved desktop edits are not visible to the API; the revision check prevents a
later stale editor save from silently overwriting an imported chapter. Server controls live in the library, reached through the workspace’s existing
save-flushing navigation. Live editor refresh during remote imports still
requires later integration. No deletion, replacement, AI execution, book upload or remote server-control
endpoint is exposed. Native UI, firewall permissions and real Sagarwar/network
interoperability require later verification.
