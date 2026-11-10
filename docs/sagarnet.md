# SagarNet client API

Sagar and other native clients use OAuth 2.0 Authorization Code with PKCE and the versioned JSON API. Use the configured SagarNet server origin with HTTPS. Local development can use HTTP on loopback. Resource paths below are relative to `/api/v1`. OAuth endpoints are relative to the server origin.

Send `Accept: application/json` on API requests. Use `Content-Type: application/json` for JSON bodies. Add `Authorization: Bearer <access_token>` when acting on behalf of a user. Public reads can omit this header.

## Register and connect a native app

1. Register the app in **Administration → OAuth apps** and copy its **Client ID**. This is a public client and has no client secret. Register the callback URI your app handles. For an IPv4 desktop loopback callback, a registration such as `http://127.0.0.1/oauth/callback` allows the app to choose an available local port. The scheme, host, path, and any query string must match the registration. A registered HTTPS callback must match exactly.
2. For each attempt, generate a fresh cryptographically random `state` and a 43–128 character PKCE verifier. A verifier made from 32 random bytes encoded as unpadded base64url is sufficient. Compute the challenge as unpadded base64url of the SHA-256 digest of the verifier string. Keep both the verifier and state in memory. Start the loopback listener before opening the system browser.
3. Open `GET /oauth/authorize` with these URL-encoded parameters.

   | Parameter | Value |
   | --- | --- |
   | `client_id` | Registered public Client ID |
   | `redirect_uri` | Full callback for this attempt, such as `http://127.0.0.1:53142/oauth/callback` |
   | `response_type` | `code` |
   | `scope` | Space-separated `sharing:read sharing:write`, or just `sharing:read` for a read-only client |
   | `state` | Random value generated for this attempt |
   | `code_challenge` | Computed PKCE challenge |
   | `code_challenge_method` | `S256` |

   The browser handles registration, email verification, sign-in, two-factor challenges, and consent. New users who register during authorization return to sign-in. Validate the returned `state` against the pending attempt before accepting the `code` or handling an OAuth `error`.

4. Send `POST /oauth/token` as `application/x-www-form-urlencoded` with `grant_type=authorization_code`, `client_id`, `code`, `code_verifier`, and `redirect_uri`. Use the exact URI from step 3, including its port. The URI must still match the current registration. Each code can be exchanged once.
5. The token response includes `token_type`, `access_token`, `expires_in` in seconds, and `refresh_token`. Fetch `GET /api/v1/me` to identify the account. Keep the access token in memory and the refresh token in the OS credential store. Exclude tokens, authorization codes, and PKCE verifiers from logs.

Access tokens are currently issued for one hour and refresh tokens for 30 days. To refresh, send `POST /oauth/token` with `grant_type=refresh_token`, `client_id`, and `refresh_token`. Serialize refreshes and replace the stored refresh token with the new one on success. If the grant was revoked or expired, clear local credentials and start sign-in again.

To disconnect, call `DELETE /api/v1/me/token` with the current bearer token. It revokes that access token and its associated refresh token and returns `204`. Clear local credentials after success or when the server confirms the token is already invalid. An administrator can revoke a client or change its redirect URI, which requires users of that client to authorize again. [OAuth for Native Apps](https://www.rfc-editor.org/rfc/rfc8252.html)

## Endpoint reference

**Optional read** permits guest access to public data. When a bearer token is present, it must be valid and have `sharing:read`. **Read** and **Write** require a verified user with the indicated scope.

| Method and path | Access | Success response |
| --- | --- | --- |
| `GET /books?q=…&page=1` | Optional read | Public catalog, plus the caller’s own private books, by title or author. Paginated book summaries. |
| `POST /books/match` | Optional read | Exact visible editions or suggested books. |
| `GET /books/{book}` | Optional read | Visible book, editions, cover URL, and caller capabilities. |
| `GET /books/{book}/cover` | Optional read | Cover image bytes, or a `302` redirect to a short-lived signed storage URL. A missing or inaccessible cover returns `404`. |
| `GET /books/{book}/editions/{edition}/chapters?page=1` | Optional read | Paginated chapters in spine order, including public translation counts. |
| `GET /languages` | Optional read | Supported Sagar base language codes and display names in `data` (`code`, `name`). |
| `GET /chapters/{chapter}/translations?target_language=my&page=1` | Optional read | Paginated published translations. The target language filter is optional. |
| `GET /translations/{translation}` | Optional read | Translation summary, contributor ID, `content`, and `content_format_version`. A private translation is readable only by its contributor. |
| `GET /me` | Read | `id`, `name`, and `email`. |
| `GET /me/translations?page=1` | Read | Paginated caller-owned translations, with book and chapter labels. |
| `GET /chapters/{chapter}/my-translation?target_language=my` | Read | Caller-owned translation summary for the required target language, or JSON `null` when absent. |
| `POST /books` | Write | `201` for a new book or `200` for a reused edition. Returns `book_id`, `edition_id`, and `visibility`. |
| `PUT /books/{book}/visibility` | Write | `204`. Book owner only. Send `{"visibility":"public"}` or `{"visibility":"private"}`. |
| `PUT /books/{book}/link` | Write | `204`. Records a confirmed link. The client stores its local book and chapter mapping. |
| `PUT /books/{book}/follow` | Write | `204`. Follows a public book. |
| `DELETE /books/{book}/follow` | Write | `204`. Removes the caller’s follow. |
| `DELETE /books/{book}` | Write | `204`. Only the original creator can delete an eligible book. |
| `PUT /chapters/{chapter}/my-translation` | Write | `201` on first save or `200` on update or an identical retry. Returns the translation summary. |
| `DELETE /chapters/{chapter}/my-translation` | Write | `204`. Deletes the caller’s server copy. Send `translation_id` and `revision` in a JSON body. |
| `DELETE /me/token` | Bearer token | `204`. Revokes the current token pair. |

`GET /languages` returns the complete catalog without pagination and advertises supported submission formats in `translation_content_format_versions`. An absent field indicates a plain-text server.

Paginated responses include `data`, `current_page`, `last_page`, `per_page`, and `total`. Chapter pages contain up to 100 items. Other lists above contain up to 25. To advance, increment `page` and preserve your original filters.

Book summaries include `id`, `title`, `author`, `description`, and `visibility`. Search results and match suggestions add `chapter_count`, the XHTML spine count of the book’s first edition. Book detail adds `can_manage`, `can_delete`, `cover_url` (a relative path or null), and `editions`. Edition entries include `id`, `fingerprint_version`, `source_fingerprint`, and `xhtml_spine_count`. Chapter entries include `id`, `spine_position`, `label`, `source_fingerprint`, and `public_translation_count`.

A translation summary includes `id`, `chapter_id`, `target_language`, `visibility`, `revision`, `published_at`, and `updated_at`. A public list adds `contributor_id` and `contributor_name`. My Shares adds `book_id`, `book_title`, `chapter_label`, and `spine_position`. Fetch the translation detail to read its text.

## Match or create a book

Send the book's `title`, optional `author`, `fingerprint_version: 1`, `source_fingerprint`, and `xhtml_spine_count` to `POST /books/match`. For example, a public match has this shape.

```json
{
  "exact": [{ "edition_id": 12, "book_id": 7, "book_visibility": "public" }],
  "suggestions": []
}
```

Exact matching uses the fingerprint version, fingerprint, and XHTML spine count. A title or author match with a different fingerprint is a suggestion. The public search endpoint lists public books. An authenticated match may also return the caller's private books. When linking a different edition manually, ask the user to confirm the local chapter mapping before applying or publishing a translation. Send the link request after confirmation. A lookup by itself leaves the book eligible for deletion.

### Fingerprint v1

Compute the fingerprint locally from the immutable EPUB snapshot.

1. Use the original OPF title and author, or an empty string for missing values. Trim and collapse whitespace runs to a single space, then lowercase using Unicode rules. Encode as UTF-8.
2. Read XHTML documents in OPF spine order. Normalize CRLF and standalone CR to LF. Compute each chapter's SHA-256 digest from those UTF-8 bytes. Send each digest as 64 lowercase hexadecimal characters.
3. Start the book hash with the UTF-8 bytes of `sagar-source-v1` followed by a zero byte. Append the normalized title and author in that order. Prefix each with its UTF-8 byte length as an unsigned 64-bit big-endian integer. Append the chapter digests as raw 32-byte values in spine order. Send the resulting SHA-256 hash as lowercase hexadecimal.
4. Send the number of XHTML spine items as `xhtml_spine_count`. This is a document count. Use the zero-based position as `spine_position`.

### Create with a manifest

Send the matching fields plus an optional `description`, optional `visibility` (defaults to `private`), and a nonempty `chapters` array. Each chapter has `spine_position`, optional `label`, and `source_fingerprint`.

```json
{
  "title": "Example book",
  "author": "Example author",
  "description": "Optional catalog description",
  "visibility": "private",
  "fingerprint_version": 1,
  "source_fingerprint": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "xhtml_spine_count": 1,
  "chapters": [
    {
      "spine_position": 0,
      "label": "Chapter 1",
      "source_fingerprint": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    }
  ]
}
```

Replace the example hashes with computed values. Title, author, and chapter labels have a 255-character limit. Description has a 10,000-character limit. The spine count and manifest support up to 5,000 entries. Positions must be unique and below the spine count.

To include a cover, use `multipart/form-data`. Send scalar fields as text, `chapters` as a JSON array encoded in a single text field, and the image file as `cover`. The cover accepts JPEG, PNG, or WebP up to 2 MB and 4,096 pixels per dimension. A reused edition returns its existing IDs and visibility. Fetch book detail for the saved metadata and confirm the destination before sharing.

## Supported translation languages

Fetch `GET /api/v1/languages` to populate the client language picker. It requires no authentication; when a bearer token is supplied, the normal read-scope checks apply. The response contains all 60 base language codes and display names from Sagar's `src/lib/languageNames.json`. This excerpt shows the response shape; the actual response includes the complete catalog.

```json
{
  "data": [
    { "code": "en", "name": "English" },
    { "code": "my", "name": "Myanmar · မြန်မာ" },
    { "code": "shn", "name": "Shan · ရှမ်း" },
    { "code": "mnw", "name": "Mon · မွန်" },
    { "code": "zh", "name": "Chinese" },
    { "code": "ja", "name": "Japanese" }
  ]
}
```

Send the language code, not its display name, in `target_language`. The field is required for every translation save and for `GET /chapters/{chapter}/my-translation`. It is optional for the public translations list; omitting it lists all public languages and contributors for that chapter.

Tags have a maximum of 63 characters: a 2–8 letter base followed by optional hyphen-separated subtags of 1–8 letters or digits. The base must be in the catalog. Script and region subtags are syntax-checked; the API does not verify them against a registered subtag list. Underscores and language display names are not accepted.

| Submitted tag | Stored tag | Meaning |
| --- | --- | --- |
| `my` | `my` | Myanmar base language |
| `my-Mymr-MM` | `my-mymr-mm` | Sagar's Myanmar picker tag |
| `shn-Mymr-MM` | `shn-mymr-mm` | Sagar's Shan picker tag |
| `zh-Hans` | `zh-hans` | Chinese with a script subtag |
| `JA` | `ja` | Case-insensitive Japanese code |

The complete normalized tag identifies a language variant. `my` and `my-mymr-mm` are separate translation keys; the server does not merge them. Language filters use exact, case-insensitive tag matching after normalization: `?target_language=my` does not include `my-mymr-mm`. Clients should use a consistent tag when saving, fetching, and updating a translation.

An unsupported base language such as `zz` returns `422` without saving anything:

```json
{
  "message": "The target language must be a language supported by Sagar.",
  "errors": {
    "target_language": [
      "The target language must be a language supported by Sagar."
    ]
  }
}
```

## Translation identity

A book groups editions, and each edition has many chapters. Each chapter can have many translations, including multiple translations in the same language from different users. The unique key is `(chapter_id, authenticated user_id, normalized target_language)`.

For example, one chapter can contain all of these independently:

| Contributor | Target language | Translation |
| --- | --- | --- |
| Alice | `my` | Alice's Myanmar version |
| Bob | `my` | Bob's Myanmar version |
| Alice | `zh` | Alice's Chinese version |
| Carol | `ja` | Carol's Japanese version |

The authenticated account determines the contributor. Do not send `user_id` to choose an owner. Each translation has its own ID, revision, content, and visibility. Another contributor's private translation remains inaccessible even to the book owner, and is excluded from public lists and counts.

## Save and manage translations

Send only the translated chapter. Keep the original EPUB, source XHTML, embedded book resources, and bilingual previews on the device. Content format version 1 is plain text and is the default when the field is omitted. Send `"content_format_version": 2` with a translated chapter HTML fragment in `content` after the server advertises version 2. Treat received HTML as untrusted and validate and sanitize it before rendering. The API accepts up to 250,000 characters of nonempty content without null bytes. Every submission must supply a target-language tag such as `my` or `en-US`. The server stores tags in lowercase. The base language must appear in `GET /languages`, matching Sagar’s `src/lib/languageNames.json` catalog. Script and region variants such as `my-Mymr-MM`, `shn-Mymr-MM`, and `zh-Hans` are accepted and preserved. Unsupported base languages return `422` on submission and language-filtered lookups.

For the first save, omit `translation_id` and `revision`. New translations default to private. Ask for explicit confirmation before sending public visibility.

```json
{
  "target_language": "my",
  "content": "ဘာသာပြန်ထားသော အခန်းစာသား",
  "visibility": "private"
}
```

A contributor can save one translation per chapter per target language. Different contributors can each save their own translations in the same or different languages. For example, `my` (Myanmar/Burmese), `zh` (Chinese), and `ja` (Japanese) are independent translations. The required `target_language` selects the translation to create or update; changing it creates a separate translation, rather than replacing another language. To update an existing translation, send the full content and target language, plus the `translation_id` and `revision` from the latest summary. Omit visibility to preserve the existing value. Use the returned revision for the next write. On a conflict, reload the server copy and ask the user to reconcile changes before retrying. Delete with the same pair of fields; only that translation ID within the chapter and authenticated account is deleted. Missing, deleted, other-chapter, or other-user IDs return `404`. A successful deletion removes the server copy while the desktop client retains its local draft.

### Read, update, and delete a language version

For chapter `42`, first read your selected language's latest summary:

```http
GET /api/v1/chapters/42/my-translation?target_language=my-Mymr-MM
Authorization: Bearer <access_token>
Accept: application/json
```

A successful response is a summary such as the following, or JSON `null` when you have no translation for that exact tag. Fetch `GET /api/v1/translations/101` to retrieve its content.

```json
{
  "id": 101,
  "chapter_id": 42,
  "target_language": "my-mymr-mm",
  "visibility": "private",
  "revision": 1,
  "published_at": null,
  "updated_at": "2026-11-09T16:00:00Z"
}
```

Update using `PUT /api/v1/chapters/42/my-translation` with the latest ID and revision:

```json
{
  "target_language": "my-Mymr-MM",
  "content": "Updated translated chapter text",
  "translation_id": 101,
  "revision": 1
}
```

A changed translation returns `200` and increments its revision, for example to `2`. An identical retry may return the existing revision. A changed payload with a stale revision returns `409`. An ID belonging to another language cannot update the selected language. To add another language, omit `translation_id` and `revision` and send its target tag as a new save.

Delete only that translation with `DELETE /api/v1/chapters/42/my-translation`:

```json
{
  "translation_id": 101,
  "revision": 2
}
```

Deletion does not require `target_language`, because the ID selects the exact record. It leaves other languages and other contributors' translations intact. A stale revision returns `409`; an unavailable ID returns `404`.

### Visibility and book deletion

Public translations require a public book. A public book can contain private translations, and its public lists and counts include only published versions. Publishing the book leaves each translation's visibility as set by its contributor. A book cannot become private while it has public translations.

The original creator can delete a book before another user first confirms a link, follows it, or saves a translation for it. A private translation counts as use. The deletion lock survives unfollowing, unlinking, and translation removal. Use the caller's `can_delete` field to decide whether to offer deletion and handle a conflict if use occurs before the delete request completes.

## Errors and retries

OAuth errors use `error` and, where available, `error_description`. API errors generally return a `message` and validation errors include an `errors` object keyed by field.

| Status | Client handling |
| --- | --- |
| `400` | Check the OAuth request, registered callback, state, and verifier. Restart authorization if the code is invalid or expired. |
| `401` | Refresh the API token once if possible. Start sign-in when the refresh grant is invalid. |
| `403` | Check email verification, token scope, or permission for the resource. |
| `404` | Treat the resource as unavailable. Private records can return this status. |
| `409` | Reload current state. Resolve a stale revision, publication restriction, fingerprint collision, or deletion lock before continuing. |
| `422` | Display field validation errors and let the user correct them. |
| `429` | Honor `Retry-After` when present and back off. |
| `503` | The operation is temporarily unavailable. Retry after backoff. |

On a connection failure during a write, reload the affected resource before retrying. Repeated book creation may return the existing edition, and an identical translation save may return its current revision. Repeated follow and unfollow requests are safe. Treat `204` as an empty response body.
