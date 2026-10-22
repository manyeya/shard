# Store listing text

What the Chrome Web Store, Edge Add-ons and Firefox Add-ons ask for, ready to paste. The steps
that use it are in [docs/STORES.md](../../docs/STORES.md); the images are beside this file. Keep
it true to `extension/manifest.json` and `extension/background.js`: when the extension changes
what it does, change this too.

## Name

```text
Shard integration
```

It's `name` in `extension/manifest.json`, which the stores read from the zip.

## Summary

```text
Hands your browser's downloads to the Shard app. When Shard isn't running, the browser downloads as usual.
```

106 characters (the Chrome Web Store allows 132). It's `description` in
`extension/manifest.json`: the Chrome Web Store and Edge Add-ons take the summary from there, and
Firefox Add-ons starts from it, so a new summary means changing the manifest.

## Description

```text
Shard integration hands the downloads you start in your browser to Shard, a free, open-source download manager for macOS, Windows and Linux. Shard fetches each file in parallel parts over several connections at once, picks up after a pause, a dropped connection or a crash, and keeps running in the tray when its window is closed.

You need the Shard app too. Get it free from https://manyeya.github.io/shard/ (its setup can open this page for you). On its own, the extension doesn't download anything.

What it does
• When a download starts, it passes the download's address, the cookies the site set for it, the page it came from and the browser's user agent to Shard on your own computer, at http://127.0.0.1:47110. That's how downloads behind a login work in Shard as well.
• When Shard takes the download, the extension cancels the browser's copy and removes it from the browser's download list, so the file isn't fetched twice.
• When Shard isn't running, or turns a download down, the extension does nothing and the browser downloads the file as usual.
• When it's installed or updated, it sends Shard an empty hello, so Shard's setup can show that it's connected.

What it doesn't do
• It never reads or changes the pages you visit, and has no content scripts.
• It sends nothing anywhere except the Shard app on your computer: no accounts, no analytics, no tracking.
• It leaves files a page makes itself (blob: and data: links) to the browser.

Shard is open source, under the MIT or Apache-2.0 licence: https://github.com/manyeya/shard
```

## Category

Suggestions; pick the nearest the store offers.

| Store | Category |
| --- | --- |
| Chrome Web Store | Tools (under Productivity) |
| Edge Add-ons | Productivity |
| Firefox Add-ons | Download Management |

Edge Add-ons also takes up to seven search terms, which people don't see:

```text
download manager, downloads, download accelerator, resume downloads, segmented download, parallel downloads, Shard
```

## Single purpose

```text
Shard integration hands downloads that start in the browser to the Shard download manager app on the same computer, with the cookies, referrer and user agent each download needs.
```

## Permission justifications

`downloads`:

```text
To notice when a download starts (downloads.onCreated) and, once the Shard app on the same computer has accepted it, to cancel the browser's copy and erase it from the download list, so the file isn't downloaded twice. When Shard isn't running, the extension leaves the download alone.
```

`cookies`:

```text
To read the cookies for the address being downloaded, at the moment the download starts, and pass them to the Shard app on the same computer, so a file behind a login downloads in Shard as it would in the browser. The extension reads only the cookies for that one address, never changes or deletes a cookie, and sends them nowhere else.
```

Host permission (`<all_urls>`):

```text
Downloads can come from any site, and the cookies permission only gives the extension a site's cookies when it has host permission for that site. The host permission also lets it hand the download to the Shard app at http://127.0.0.1:47110. The extension has no content scripts and never reads or changes the pages you visit.
```

Remote code: **No**. All the extension's code is `background.js`, in the package.

## Data use

The Chrome Web Store and Edge Add-ons ask what user data the extension collects. It never sends
anything off the computer, but it does read two of the kinds of data the form lists, so declare
them:

- **Authentication information**: the cookies for a download's address.
- **Web history**: the download's address and the page it came from (the referrer).

Leave the rest unticked (personally identifiable information, health, financial and payment
information, personal communications, location, user activity, website content), and tick all
three certifications: the data isn't sold or transferred to third parties, isn't used or
transferred for anything unrelated to the single purpose, and isn't used to determine
creditworthiness or for lending.

Where a store asks for more detail:

```text
The extension handles this data only on the user's computer. When a download starts, it sends the download's address, the cookies for that address, the referring page and the browser's user agent to the Shard app at http://127.0.0.1:47110, an address only that computer can reach, and nowhere else. It has no servers, no analytics and no remote code. Shard deletes a download's cookies when the download finishes or is removed.
```

Firefox Add-ons doesn't ask: the Firefox build's manifest declares
`"data_collection_permissions": {"required": ["none"]}`, and the listing shows that.

## URLs

| Field | URL |
| --- | --- |
| Homepage | <https://manyeya.github.io/shard/> |
| Support | <https://github.com/manyeya/shard/issues> |
| Questions | <https://github.com/manyeya/shard/discussions> |
| Privacy policy | <https://manyeya.github.io/shard/privacy.html> |
| Source | <https://github.com/manyeya/shard/tree/main/extension> |

Licence, where a store asks: MIT or Apache-2.0 (Firefox Add-ons takes one; choose MIT).

## Note for reviewers

```text
This extension hands downloads to Shard, a free, open-source desktop download manager (https://github.com/manyeya/shard). It needs the Shard app on the same computer; without it, the extension does nothing and the browser keeps its downloads.

To test it:
1. Install Shard from https://github.com/manyeya/shard/releases/latest: the .dmg on macOS, Shard-Setup-<version>-windows-x64.exe on Windows, or the .AppImage on Linux. Open it and follow its setup; its browser step can be skipped. The app isn't signed yet: on macOS choose System Settings > Privacy & Security > Open Anyway; on Windows, if SmartScreen appears, choose More info > Run anyway.
2. Check Shard is running: http://127.0.0.1:47110/ping shows "shard".
3. With the extension installed, click any download link on any site. The download disappears from the browser's download list and appears in Shard's window, where it downloads.
4. Quit Shard (its tray or menu bar icon > Quit Shard) and click the link again: the browser downloads the file itself.

The extension's only requests go to http://127.0.0.1:47110 on the same computer: POST /add with the download's url, cookies, referrer and user agent when a download starts, and an empty POST /hello when the extension is installed or updated. All of its code is background.js in the package, unminified; the source is at https://github.com/manyeya/shard/tree/main/extension.
```
