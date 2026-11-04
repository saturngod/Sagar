# Sagar Content Server REST API Guide

API version: **v1**. This guide describes the implemented Sagar desktop server
for the future Sagarwar client. There is no reset/delete API.

Open `/docs` on the server address in a browser to read this documentation. That
page is public and contains no library data, even when API authentication is
enabled. API routes retain authentication and browser-origin restrictions.

The server root (`/`) is a **read-only web reader**: browse and search books, then
read each chapter's original and saved translation side by side. It uses the same
username and password (the browser asks for them) and cannot edit, import or
download anything. Its data routes live under `/reader/api/` and are an internal
detail of the page, not part of this v1 API: they accept same-origin browser GET
requests only, reject cross-site requests and non-local host names, return
sanitized chapter HTML, and serve only book images (no stylesheets, fonts,
documents or EPUB downloads). Opening the reader never changes saved translations.

## 1. Connection and scope

Use the desktop's LAN IPv4 address and the port returned when the server starts:

```text
http://192.168.1.20:8080
```

The address and port above are examples, not built-in defaults. The listener
binds `0.0.0.0` (IPv4). Port `0` selects an available port. All API endpoints
start with `/api/v1`; `/` (web reader) and `/docs` (this guide) are browser
pages. Paths are case-sensitive; use the routes exactly as listed.

The server shares the entire local Sagar library while running. It supports
book browsing, covers, downloading original EPUBs, and exchanging saved chapter
translations. It does not upload/import new books into Sagar, delete data,
overwrite existing translations, run AI, or control the server remotely.

The server is off at application launch. In the Sagar library, open **Settings →
Content server**, enter the port and optional credentials, then select **Start
server**. The settings screen defaults to port 8080; 0 selects an available port.
While running, it shows local-network connection addresses and **Stop server**.
Reopen this section to check the running state. The Sagarwar client is not yet
implemented.
No automatic discovery, QR pairing, TLS, or public hosting is provided.

## 2. Authentication and common headers

### Optional username and password

- Both empty: authentication is disabled.
- Both supplied: every API route requires HTTP Basic authentication.
- Only one supplied: server startup is rejected.
- Username cannot contain `:`. Neither credential can contain control characters.
- Username is limited to 256 UTF-8 bytes; password to 1,024 UTF-8 bytes.
- Credentials live in memory and are not returned in status or persisted.

Authenticated request header:

```http
Authorization: Basic <base64 of UTF-8 username:password>
```

With curl, `--user reader` prompts for the password without putting it directly
in the command. Add this option to **every** example below when authentication
is enabled, including EPUB and cover downloads:

```bash
curl --user reader --fail-with-body \
  'http://192.168.1.20:8080/api/v1'
```

Plain HTTP does not encrypt Basic credentials or book content. Use a trusted
local network; do not expose the server port to the public internet. Without
credentials, any reachable native client can download books and fill empty
translations.

### Request conventions

| Header | Use |
| --- | --- |
| `Authorization` | Required on every route when credentials are configured |
| `Accept: application/json` | Appropriate for metadata and translation responses |
| `Content-Type: application/json` | Required for translation PUT bodies |

Do not send browser `Origin` or `Sec-Fetch-Site` headers. Requests containing
either are rejected with 403, even when authentication is disabled. No CORS is
enabled; this API is intended for native clients. `Accept` does not implement
content negotiation; each route returns its documented representation.

Responses passing through the API middleware include:

```http
Cache-Control: no-store
X-Content-Type-Options: nosniff
```

JSON responses use `application/json`. Routing/extraction failures may return
plain text. Always check the HTTP status before decoding the response.

## 3. Endpoint index

| Method | Path | Success response |
| --- | --- | --- |
| GET | `/api/v1` | Server capabilities |
| GET | `/api/v1/books` | Book array with title, author and cover URL |
| GET | `/api/v1/books/{bookId}/cover` | Cover image bytes |
| GET | `/api/v1/books/{bookId}/epub` | Unchanged original EPUB bytes |
| GET | `/api/v1/books/{bookId}/chapters` | Spine-ordered chapters with saved HTML |
| GET | `/api/v1/books/{bookId}/chapters/translated` | IDs of chapters with saved translation HTML |
| PUT | `/api/v1/books/{bookId}/chapters/{chapterId}/translation` | Import into an empty chapter, or skip existing content |

All successful requests below return **200 OK**, including a PUT that skips an
existing translation. There is no pagination in v1. Book search is supported through the optional `q` query parameter.

## 4. GET `/api/v1` — server information

**Request:** no parameters or body. Authentication applies when enabled.

```bash
curl --fail-with-body -H 'Accept: application/json' \
  'http://192.168.1.20:8080/api/v1'
```

**Response:**

```json
{
  "name": "Sagar",
  "apiVersion": 1,
  "translationPolicy": "fillMissing",
  "htmlFormat": "chapter-body-fragment"
}
```

| Field | Type | Meaning |
| --- | --- | --- |
| `name` | string | Server application name |
| `apiVersion` | integer | API contract version |
| `translationPolicy` | string | `fillMissing`: existing translations are protected |
| `htmlFormat` | string | HTML is a chapter body fragment, not a full XHTML document |

## 5. GET `/api/v1/books` — library

**Request:** optional `q` query parameter; no body. Omit `q` to list all books.
Search matches a literal substring in either title or author, using Unicode
lowercase comparison. Leading/trailing query whitespace is trimmed; an empty
or whitespace-only query returns all books. `%` and `_` are literal characters,
not wildcards. Accents and Unicode normalization are not folded. No chapter
content is searched. Authentication applies as usual.

```bash
curl --fail-with-body --get -H 'Accept: application/json' \
  --data-urlencode 'q=မြန်မာ' \
  'http://192.168.1.20:8080/api/v1/books'
```

Search returns the same book array and ordering as the unfiltered endpoint;
no matches returns `[]`. URL-encode query values (including spaces and `&`).

```bash
curl --fail-with-body -H 'Accept: application/json' \
  'http://192.168.1.20:8080/api/v1/books'
```

**Response:** an array ordered by most recently updated, then book ID. An empty
library returns `[]`.

```json
[
  {
    "id": "book-123",
    "sourceSha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "title": "Book A",
    "author": "Author XXX",
    "coverUrl": "/api/v1/books/book-123/cover",
    "sourceLanguage": "en",
    "targetLanguage": "my"
  }
]
```

The IDs and hash above are illustrative; use values from the live response.

| Field | Type | Meaning |
| --- | --- | --- |
| `id` | string | Server-local project ID; use in subsequent paths |
| `sourceSha256` | string | SHA-256 of the original EPUB bytes; cross-app book identity |
| `title` | string | Saved project title for display |
| `author` | string or null | Saved author name; null if unavailable |
| `coverUrl` | string | Origin-relative cover route, advertised even if no cover exists |
| `sourceLanguage` | string or null | Saved source language |
| `targetLanguage` | string | Saved translation language; must match exactly on upload |

Display the title, author and separately fetched cover. Resolve `coverUrl`
against the **server origin**, not `/api/v1/books`. Do not embed the credentials
in the URL. The list contains no image bytes, filesystem paths or original
EPUB contents.

Titles/authors help users select books. Match sync identity using
`sourceSha256` plus `targetLanguage`, not title/author or server-local ID.

## 6. GET `/api/v1/books/{bookId}/cover` — cover image

**Path parameter:** `bookId`, the book's `id` from the library response.
No body or query parameters.

```bash
curl --fail-with-body \
  'http://192.168.1.20:8080/api/v1/books/book-123/cover' \
  --output cover.image
```

**Response:** raw image bytes, not JSON or a base64 wrapper.

```http
HTTP/1.1 200 OK
Content-Type: image/png
Cache-Control: no-store
X-Content-Type-Options: nosniff
Content-Security-Policy: sandbox; default-src 'none'; style-src 'unsafe-inline'

<binary image bytes>
```

`Content-Type` reflects the selected image. Supported media types are
`image/png`, `image/jpeg`, `image/gif`, `image/webp`, `image/avif`, `image/bmp`
and `image/svg+xml`. The CSP header is applied to all cover responses, including
SVG. Load covers as images; never execute them as privileged documents.

Cover selection:

1. Custom cover saved through Sagar Book Settings, when present.
2. Original cover declared in the source EPUB, otherwise.

This uses the existing library cover loader and archive safety checks. Original
cover payloads larger than 4 MiB are unavailable through this route. Invalid
custom-cover data or archive I/O failures can produce errors rather than fallback.

**404:** missing book, absent/unavailable cover, or unsupported cover media type.
Show a placeholder for unavailable covers. Retry server/transfer failures
according to the error section. A custom cover does not change the original
EPUB download or its hash.

## 7. GET `/api/v1/books/{bookId}/epub` — add/download a book

**Path parameter:** `bookId`. No request body or query parameters.

```bash
curl --fail-with-body \
  'http://192.168.1.20:8080/api/v1/books/book-123/epub' \
  --output book.epub
```

**Response:** streamed original EPUB bytes.

```http
HTTP/1.1 200 OK
Content-Type: application/epub+zip
Content-Disposition: attachment; filename="book.epub"
Cache-Control: no-store
X-Content-Type-Options: nosniff

<binary EPUB bytes>
```

This is the private immutable source snapshot, **not a translated EPUB**. It
excludes saved translations and custom-cover changes. Verify the downloaded
file's SHA-256 against `sourceSha256` before importing it. Fetch translations
separately using the chapter endpoint after local import.

**404:** book does not exist. A missing/unreadable stored file returns a server
error. A download may fail after response headers; verify completeness and hash
before accepting it. Range requests and resumable downloads are not implemented.

## 8. GET `/api/v1/books/{bookId}/chapters` — translation inventory

**Path parameter:** `bookId`. No request body or query parameters.

```bash
curl --fail-with-body -H 'Accept: application/json' \
  'http://192.168.1.20:8080/api/v1/books/book-123/chapters'
```

**Response:** chapter array in OPF spine order, including saved translation HTML.
An existing book with no chapters returns `[]`. Missing books return 404.

```json
[
  {
    "id": "chapter-001",
    "href": "EPUB/Text/chapter01.xhtml",
    "title": "Chapter One",
    "spineIndex": 0,
    "document": {
      "html": "<h1>အခန်း ၁</h1><p>မြန်မာစာ</p>",
      "status": "draft",
      "revision": 3,
      "authored": true
    }
  },
  {
    "id": "chapter-002",
    "href": "EPUB/Text/chapter02.xhtml",
    "title": "Chapter Two",
    "spineIndex": 1,
    "document": {
      "html": null,
      "status": "pending",
      "revision": 0,
      "authored": false
    }
  }
]
```

| Field | Type | Meaning |
| --- | --- | --- |
| `id` | string | Server-local chapter ID, required for upload path |
| `href` | string | Original XHTML resource path inside the EPUB |
| `title` | string | Display chapter label |
| `spineIndex` | integer | Reading-order position, starting at zero |
| `document.html` | string or null | Saved translation body HTML; null when absent |
| `document.status` | string | `pending`, `draft`, `reviewed`, or `skipped` |
| `document.revision` | integer | Saved revision; send as `expectedRevision` when uploading |
| `document.authored` | boolean | Whether saved through the chapter document workflow; not a presence/completion flag |

Legacy block translations are adapted to chapter HTML by the existing editor
storage layer. Do not infer an empty chapter from `authored: false` or from
status alone. Protect any nonempty HTML and explicit `skipped` choice. The
server also protects legacy translated/skipped blocks during PUT, so the
server's final skip decision is authoritative.

### HTML handling and chapter mapping

HTML is a **body fragment**, not a full XHTML document. Resolve relative image,
stylesheet-related and link references using the chapter's `href` and the
original EPUB resource layout. The response does not bundle images separately.
Preserve IDs, links, inline markup, meaningful whitespace and Unicode text.
Render untrusted HTML only in a sanitized/contained reader without privileged
APIs, scripts or uncontrolled network access.

Use the exact original hash, target language and `href` to match a chapter.
A TOC entry is not necessarily one resource. Do not match by chapter label,
array index or chapter number alone. A Sagarwar chapter spanning several source
resources must be mapped/split into the corresponding per-resource fragments
before upload. Different editions or incompatible grouping are not automatically
converted by this API.

### 8.1 GET `/api/v1/books/{bookId}/chapters/translated` — translated IDs

Use this lightweight inventory to get the translated chapter ID set in a single
request, without downloading or parsing every chapter's HTML.

**Path parameter:** `bookId`, from GET books. No query parameters or body.
Authentication and common response headers apply exactly as for other routes.

```bash
curl --fail-with-body -H 'Accept: application/json' \
  'http://192.168.1.20:8080/api/v1/books/book-123/chapters/translated'
```

**200 OK response:**

```json
{
  "chapterIds": ["chapter-001", "chapter-003", "chapter-008"]
}
```

| Field | Type | Meaning |
| --- | --- | --- |
| `chapterIds` | array of strings | Server-local chapter IDs with saved, nonempty translation HTML, in spine order |

An existing book with no saved translations returns:

```json
{
  "chapterIds": []
}
```

A missing book returns **404** (`not_found`). The route is spelled `translated`;
there is no misspelling alias.

Drafts and reviewed translations are included: presence does not mean a chapter
is complete or reviewed. Legacy block translations are included when the
chapter document loader would expose them as chapter HTML. A saved chapter
document takes precedence over older block records, including a cleared
chapter document. Empty chapters and contentless Keep original choices are not
included. This endpoint does not create document rows, return HTML, mutate
revisions, or include chapter IDs from other books.

For sync, put `chapterIds` into a set to identify server chapters already
containing translations. IDs are server-local, so retain the source-resource
mapping from GET chapters; do not compare them directly with Sagarwar IDs.
The ID list alone cannot identify contentless Keep original choices. Continue
to respect statuses from GET chapters and the authoritative PUT skip result.
Fetch GET chapters when HTML or revisions are needed. All inventory data is a
point-in-time view; another editor or sync request may change a chapter later.
The existing atomic fill-only PUT check still protects against overwriting it.

## 9. PUT `/api/v1/books/{bookId}/chapters/{chapterId}/translation`

Fills a missing chapter translation. This endpoint never replaces existing
translated content, even when the incoming version differs or is newer.

**Path parameters:**

| Parameter | Source |
| --- | --- |
| `bookId` | `id` from GET books |
| `chapterId` | `id` from that book's GET chapters response |

**Request headers:** `Content-Type: application/json`; authentication when enabled.
**Request body:** UTF-8 JSON, maximum **8 MiB** including JSON overhead.
Unknown fields are rejected.

Save the following as `translation.json`, replacing the sample identity and
HTML with real values:

```json
{
  "sourceSha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "targetLanguage": "my",
  "href": "EPUB/Text/chapter02.xhtml",
  "html": "<h1>အခန်း ၂</h1><p>ဘာသာပြန်ထားသော စာသား</p>",
  "expectedRevision": 0
}
```

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `sourceSha256` | string | Yes | Exact match to selected book's original EPUB hash |
| `targetLanguage` | string | Yes | Exact match to saved target language |
| `href` | string | Yes | Exact match to the selected chapter's resource path |
| `html` | string | Yes | Nonempty translated body fragment; existing editor HTML validation applies |
| `expectedRevision` | integer | Yes | Current saved chapter revision from GET chapters |

```bash
curl --fail-with-body --request PUT \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json' \
  --data-binary @translation.json \
  'http://192.168.1.20:8080/api/v1/books/book-123/chapters/chapter-002/translation'
```

### Response: imported

```http
HTTP/1.1 200 OK
Content-Type: application/json
```

```json
{
  "outcome": "imported",
  "revision": 1,
  "status": "draft"
}
```

The save uses the existing transactional chapter revision service and updates
project/chapter progress. All imports arrive as **drafts**, regardless of source
review status. They are not automatically reviewed or approved for completed
export. The source EPUB and its stored snapshot remain unchanged.

### Response: existing translation or keep-original choice

```json
{
  "outcome": "skippedExisting"
}
```

Status is **200 OK**. Nothing is overwritten and the revision is unchanged.
This applies to nonempty saved chapter HTML, legacy translated blocks, and
chapter/block Keep original choices, including unfinished drafts.

Book/chapter identity is checked first. For a matching destination that already
has content, skipping happens before incoming HTML/revision validation. A skip
therefore does **not** certify the uploaded HTML as valid.

### Errors and retry rules

- **404:** book missing, or chapter does not belong to that book.
- **409 `identity_mismatch`:** original hash, target language or resource differs.
  Stop and resolve mapping; never guess a destination.
- **409 `revision_conflict`:** still-empty chapter has a different saved revision.
  Fetch the latest chapters and reconcile again.
- **422:** empty translated text or HTML rejected by the editor validation path.
  Preserve local HTML, report the failure, and correct it before retrying.
- **413:** JSON body too large. Do not send partial chapter content as a workaround.

Passive unsupported or malformed markup can be retained by the same permissive
path as the current chapter editor; active content is rejected. Import success
does not guarantee Rich-editor round-trip support or EPUB conformance. Existing
editor validation limits apply in addition to the HTTP body limit.

The identity check, destination check and save occur under one SQLite write
transaction. Concurrent uploads fill an empty chapter once; subsequent requests
skip. If a connection drops or times out, the save might already have committed.
Refresh the chapter inventory before retrying. A repeated successful upload
safely returns `skippedExisting`.

## 10. Error responses and limits

Domain errors normally return JSON:

```json
{
  "code": "identity_mismatch",
  "message": "Request could not be applied. Check book, chapter, language, revision and HTML."
}
```

Internal/database errors are masked; no raw book text or storage paths are
returned. Server errors normally use the message
`Library operation failed. Retry later.`

Middleware errors contain a code without a message, for example:

```json
{
  "code": "unauthorized"
}
```

| HTTP status | Example code / cause | Client action |
| --- | --- | --- |
| 400 | Malformed JSON/path extraction | Correct request; framework response may be plain text |
| 401 | `unauthorized` | Supply correct credentials; includes `WWW-Authenticate: Basic realm="Sagar", charset="UTF-8"` |
| 403 | `browser_not_allowed` | Use native HTTP client without browser-origin headers |
| 404 | `not_found`; unmatched route | Check IDs/path; use cover placeholder when appropriate |
| 405 | Unsupported method | Use a documented method |
| 408 | `request_timeout` | Refresh saved state before retrying writes |
| 409 | `identity_mismatch`, `revision_conflict` | Reconcile identity/revision; do not force overwrite |
| 413 | Request body too large | Reduce payload only without losing chapter content; otherwise report unsupported transfer |
| 415 | Wrong/missing JSON content type | Set `Content-Type: application/json` |
| 422 | JSON type/missing/unknown fields; invalid or empty translation | Correct request/content; JSON extraction errors may be plain text |
| 500 | `server_error`, `storage_error`, `io_error` | Retain local work and retry later |
| 503 | `storage_busy`, `server_busy` | Back off and retry; refresh writes first |

The middleware permits eight concurrent handlers. A handler/body-read deadline
of 30 seconds returns 408; an EPUB body streams separately after headers, so use
client download deadlines too. A timed-out blocking storage task can still finish;
a timeout is not proof of rollback. Requests rejected before body reading, such
as missing authentication, can return their error before size/type validation.

## 11. Sagarwar add-book and two-way sync flow

### Add book from Sagar

1. Connect to the server and GET `/api/v1` to check `apiVersion`.
2. GET books; display title, author and cover. Use a placeholder for missing covers.
3. Select a book. Download its original EPUB, verify the original hash and import
   it through Sagarwar's normal safe import flow.
4. GET chapters. Map resource paths to the imported source; copy available
   translations only into empty local destinations, keeping existing choices.
5. Save imported HTML as drafts, preserve resource references, and show results.

Do not assume the source EPUB contains translations or custom-cover changes.
A custom library cover can be shown/stored separately by the future client.

### Sync an existing book

Flush pending local edits first and match original EPUB hash plus target language.
Use GET `/api/v1/books/{bookId}/chapters/translated` for the translated ID set;
fetch GET chapters for resource mappings, statuses, HTML and upload revisions.
Then reconcile each mapped resource:

| Sagar (server) | Sagarwar (client) | Action |
| --- | --- | --- |
| Translation exists | Empty | Validate and save server HTML locally, only if still empty |
| Empty | Translation exists | PUT local HTML with the server revision |
| Translation exists | Translation exists | Skip both, including differing drafts |
| Empty | Empty | Skip |
| Either side explicitly keeps original | Any | Preserve that side's choice; do not replace it |

The client must implement its own transactional fill-only and revision checks
for downloads. This server enforces protection only for writes into Sagar.
Do not mark source translations deleted after transfer. Show downloaded,
uploaded, skipped and failed counts, preserving recoverable local input.

There is no single batch-sync or distributed all-or-nothing transaction.
Partial success is expected; retry reconciliation fills remaining chapters.
Unsaved desktop typing is not exposed by the server. A later stale editor save
is rejected by the revision service; the server controls are available from the library after leaving the workspace
through its save-flushing navigation. Live editor updates during remote imports
remain a separate integration task. Re-fetch when identities, languages or revisions
change during sync.

## 12. Local server lifecycle — Tauri only

These are local app commands, **not HTTP endpoints**. The typed wrapper is
[`src/lib/tauri/contentServer.ts`](../src/lib/tauri/contentServer.ts).

### Start

```typescript
import { invoke } from "@tauri-apps/api/core";

const status = await invoke("start_content_server", {
  config: {
    port: 8080,
    username: "",
    password: "",
  },
});
```

Configuration fields default to `port: 0`, `username: ""`, `password: ""` when
omitted inside `config`. Port is an unsigned 16-bit integer (0–65535). Unknown
configuration fields are rejected.

```json
{
  "running": true,
  "port": 8080,
  "authenticationRequired": false,
  "urls": ["http://192.168.1.20:8080"]
}
```

`urls` contains discovered non-loopback IPv4 connection addresses. The list can
be empty if no network address can be discovered; the UI then explains how to
use the computer’s IPv4 address and port. VPN/virtual adapter addresses can also
appear; choose the address reachable from the other device. Loopback is shown
separately for testing on the desktop only.

With both credentials configured, `authenticationRequired` is true. Startup
errors include `invalid_credentials`, `already_running`, `server_bind_failed`,
and storage/server errors. Stop before changing configuration.

### Status and stop

```typescript
await invoke("content_server_status");
await invoke("stop_content_server");
```

Stopped status:

```json
{
  "running": false,
  "port": null,
  "authenticationRequired": false,
  "urls": []
}
```

Stop is idempotent. Closing Sagar ends the listener. Settings and credentials
are not automatically restored at next launch.

## 13. Implementation and verification boundaries

- HTTP routing/authentication/lifecycle: `src-tauri/src/services/content_server.rs`.
- Book/chapter queries and transactional import: `src-tauri/src/storage/content_server.rs`.
- Existing cover loader: `src-tauri/src/services/projects.rs`.
- Existing chapter save/HTML rules: `src-tauri/src/storage/documents.rs`.
- Local command adapters: `src-tauri/src/commands/mod.rs`.
- Read-only web reader routes and guard: `src-tauri/src/services/web_reader.rs`;
  page assets in `src-tauri/src/services/web_reader/`.

Web reader tests cover login, sanitized original/translated HTML, image-only
resources, path traversal, GET-only routes, cross-site and non-local host
rejection, and that reading creates no chapter document rows.

Loopback tests cover authentication, no-auth mode, downloads, cover metadata,
original/custom/missing covers, SVG response containment, identity/revision
checks, invalid/empty HTML, request size, Myanmar persistence, retries and
concurrent fill-only imports. Translated-ID inventory coverage includes spine
order, drafts, legacy fallback, cleared/empty chapters, Keep original choices,
missing books, and no document-row creation. These tests do not establish physical Sagarwar
interoperability, firewall access, Windows/Linux network behavior, native cover
rendering, or full EPUB conformance. The settings UI is implemented and browser-fixture verified. Native startup,
firewall prompts and Sagarwar interoperability remain to be verified; the
Sagarwar client is the next implementation task.
