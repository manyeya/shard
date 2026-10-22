# Getting the extension into the stores

The browser extension reaches people through the browsers' own stores: the Chrome Web Store
(Chrome, Arc, Brave, Vivaldi, Opera, Chromium, and Edge until it has its own listing), Edge
Add-ons and Firefox Add-ons. This is the owner's step-by-step for listing it in each store the
first time, and for setting up the credentials that let the release workflow send every later
version on its own.

## Contents

- [How it fits together](#how-it-fits-together)
- [Before you start](#before-you-start)
- [The Chrome Web Store](#the-chrome-web-store)
- [Edge Add-ons](#edge-add-ons)
- [Firefox Add-ons](#firefox-add-ons)
- [After a store's listing is live](#after-a-stores-listing-is-live)

## How it fits together

- One source, `extension/`, is packaged by `scripts/package-extension.sh` into
  `shard-extension-X.Y.Z-chrome.zip` (for the Chrome Web Store and Edge Add-ons) and
  `shard-extension-X.Y.Z-firefox.zip` (for Firefox Add-ons). Every release carries both.
- Each store needs its **first submission made by hand**, in its own dashboard, where the
  listing's text, images and privacy answers are filled in. The workflow only updates a listing
  that exists.
- After that, once each tagged release is out, the release workflow sends it to every store
  whose credentials are set, one job per store ([RELEASING.md](RELEASING.md#the-browser-stores)).
- Setup only offers a store once Shard's builds know its ID there. The IDs are the repository
  variables `CHROME_STORE_ID`, `EDGE_STORE_ID` and `FIREFOX_STORE_SLUG`, compiled into each
  build ([RELEASING.md](RELEASING.md#variables)). With none set, setup has no extension step.

A store only takes a version higher than the one it has. So the simplest order is: release a
version, submit that release's zip to a store by hand, and once the listing is live set its
variable and secrets. The next release is then the first that setup offers the store in, and the
first the workflow uploads on its own. If a store's credentials are already set when the
workflow releases a version that store has (say you submitted a dry run's zip of X.Y.Z by hand,
then tagged `vX.Y.Z`), the store refuses it and that store's job fails; the release and the
other stores aren't affected.

## Before you start

**The zips.** Take them from a release (`gh release download vX.Y.Z -R manyeya/shard -p
'shard-extension-*'`), from a dry run's `extension` artifact, or build them yourself:

```sh
scripts/package-extension.sh X.Y.Z dist     # needs python3 and zip
```

**The privacy policy.** Every store asks for one: <https://manyeya.github.io/shard/privacy.html>.
It's `site/privacy.html`, which the Pages workflow publishes from `main`, so check the link opens
before you submit.

**The text.** Everything the listings ask for, ready to paste, is in
[packaging/stores/LISTING.md](../packaging/stores/LISTING.md): the summary, the description, the
single-purpose statement, a justification for each permission, the data-use answers, the URLs and
a note for reviewers.

**The artwork.** Rendered from `packaging/art/art.html` by `scripts/render-art.sh` (see
[BUILDING.md](BUILDING.md#the-artwork)) and committed:

| File | Size | Chrome Web Store | Edge Add-ons | Firefox Add-ons |
| --- | --- | --- | --- | --- |
| `extension/icons/shard-128.png` | 128×128 | Store icon | Extension logo | Taken from the zip |
| `packaging/stores/screenshot-1.png` | 1280×800 | Screenshot | Screenshot | Screenshot |
| `packaging/stores/screenshot-2.png` | 1280×800 | Screenshot | Screenshot | Screenshot |
| `packaging/stores/promo-small.png` | 440×280 | Small promo tile | Small promotional tile | |
| `packaging/stores/promo-marquee.png` | 1400×560 | Marquee promo tile | Large promotional tile | |

`packaging/stores/social-preview.png` (1280×640) is the repository's social preview on GitHub
(Settings › General › Social preview), not a store image.

## The Chrome Web Store

### 1. Register

Sign in to the [Chrome Web Store developer dashboard](https://chrome.google.com/webstore/devconsole)
with the Google account that will own the listing, and pay the one-time US$5 registration fee.
Fill in the Account page (the publisher name people see, a verified contact address, and the
trader question).

### 2. Upload the first version

Choose **New item** and upload `shard-extension-X.Y.Z-chrome.zip`. The dashboard creates the
item and shows its ID, 32 letters, on the item's page and in its address. Write it down: it's
`CHROME_STORE_ID`.

### 3. The store listing tab

- **Description:** the full description from `LISTING.md`. The name and the summary come from
  the zip's manifest (`name` and `description` in `extension/manifest.json`).
- **Category:** see `LISTING.md`. **Language:** English.
- **Store icon:** `extension/icons/shard-128.png`.
- **Screenshots:** `screenshot-1.png` and `screenshot-2.png`.
- **Small promo tile:** `promo-small.png`. **Marquee promo tile:** `promo-marquee.png`.
- **Homepage URL** and **Support URL:** from `LISTING.md`.

### 4. The privacy tab

- **Single purpose:** the statement in `LISTING.md`.
- **Permission justification:** one each for `downloads`, `cookies` and the host permission
  (`<all_urls>`), from `LISTING.md`.
- **Are you using remote code?** No.
- **Data usage:** tick the kinds of data `LISTING.md` lists, and all three certifications. The
  extension handles that data only on the person's computer: it sends it to the Shard app at
  `127.0.0.1:47110` and nowhere else.
- **Privacy policy URL:** `https://manyeya.github.io/shard/privacy.html`.

### 5. Distribution and submission

On the distribution tab, choose Public and all regions. If the dashboard has a test instructions
section, paste the reviewer note from `LISTING.md` there. Then **Submit for review**. When the
review passes, the listing is live at
`https://chromewebstore.google.com/detail/<CHROME_STORE_ID>`.

### 6. The variable and the credentials

Once the listing is live:

```sh
gh variable set CHROME_STORE_ID -R manyeya/shard --body <the 32-letter ID>
```

The upload needs an OAuth client in a Google Cloud project with the Chrome Web Store API turned
on, and a refresh token for the account that owns the item. The
[chrome-webstore-upload-keys guide](https://github.com/fregante/chrome-webstore-upload-keys)
walks through making the project, enabling the API, creating a Desktop app OAuth client (which
gives the client ID and secret), and running `npx chrome-webstore-upload-keys`, which signs you in
and prints the refresh token. Google's OAuth consent screen offers an Internal audience only to
Google Workspace accounts; with a personal account choose External, and set the app to In
production, because Google expires the refresh tokens of apps left in Testing after seven days.

```sh
gh secret set CWS_CLIENT_ID -R manyeya/shard
gh secret set CWS_CLIENT_SECRET -R manyeya/shard
gh secret set CWS_REFRESH_TOKEN -R manyeya/shard
```

(`gh secret set` asks for each value.) `CWS_EXTENSION_ID`, which older notes mention, isn't used
any more.

### Automatic uploads to the Chrome Web Store

The `chrome-web-store` job uploads with version 4 of chrome-webstore-upload-cli, which uses
[version 2 of the Chrome Web Store API](https://developer.chrome.com/blog/cws-api-v2). That API
also wants your publisher ID, shown in the developer dashboard under Publisher › Settings. It
isn't secret; set it as a variable:

```sh
gh variable set CWS_PUBLISHER_ID -R manyeya/shard --body <publisher id>
```

Without it, or without `CWS_REFRESH_TOKEN`, the job does nothing, and you upload each new version
by hand in the dashboard; `CHROME_STORE_ID` can still be set, for setup.

## Edge Add-ons

### 1. Register

Sign in to [Partner Center](https://partner.microsoft.com/dashboard/microsoftedge/public/login?ref=dd)
and register for the Microsoft Edge program. It's free.

### 2. Submit the first version by hand

1. In the Edge workspace, choose **Create new extension** and upload
   `shard-extension-X.Y.Z-chrome.zip` (Edge takes the same build as Chrome).
2. **Availability:** Public, all markets.
3. **Properties:** the category from `LISTING.md`, the homepage as **Website**, and the support
   URL as **Support contact detail**.
4. **Privacy:** the single-purpose statement, a justification for each permission, "No, I am not
   using remote code", the data-usage answers and certifications from `LISTING.md`, and the
   privacy policy URL.
5. **Store listings**, English: the full description, `extension/icons/shard-128.png` as the
   extension logo (128×128 is the smallest Edge takes), `promo-small.png` as the small
   promotional tile, `promo-marquee.png` as the large one, and both screenshots. Search terms
   are optional; `LISTING.md` suggests some.
6. **Publish**, pasting the reviewer note from `LISTING.md` into **Notes for certification**.
   Microsoft says certification can take up to seven business days.

### 3. The IDs and the credentials

Edge has two different IDs:

- the **product ID**, a GUID on the extension's overview page in Partner Center (and in its
  address), which the API uploads to;
- the **store ID** at the end of the public listing's address,
  `https://microsoftedge.microsoft.com/addons/detail/<id>`, which setup links to.

For the API: in Partner Center, under Microsoft Edge, open **Publish API**, turn on the new
experience if it offers that, and choose **Create API credentials**. It shows a Client ID and an
API key; API keys have an expiry date there, so make a new one before it runs out.

Once the listing is live:

```sh
gh variable set EDGE_STORE_ID -R manyeya/shard --body <the ID from the listing's address>
gh secret set EDGE_PRODUCT_ID -R manyeya/shard
gh secret set EDGE_CLIENT_ID -R manyeya/shard
gh secret set EDGE_API_KEY -R manyeya/shard
```

Until `EDGE_STORE_ID` is set, setup sends Edge to the Chrome Web Store, which Edge can install
from too.

## Firefox Add-ons

### 1. Make an account

Sign in to the [Add-on Developer Hub](https://addons.mozilla.org/developers/) with a Firefox
account. It's free.

### 2. Submit the first version by hand

1. Choose **Submit a New Add-on**, and when it asks how to distribute the add-on, choose **On
   this site** (a listed add-on). The workflow submits to the listed channel, so the add-on has to
   be listed.
2. Upload `shard-extension-X.Y.Z-firefox.zip`, the Firefox build: its manifest has the add-on ID
   `shard-integration@manyeya.github.io`, which every later version has to keep, needs Firefox
   128 or later, and declares that it collects no data. The code isn't minified or bundled, so
   there's no source code to upload separately.
3. Describe it: the name, the **add-on URL** (the slug; `shard-integration` reads well), the
   summary and description from `LISTING.md`, the category, the homepage and support URLs, the
   privacy policy, a licence (Shard is MIT or Apache-2.0), and the reviewer note in **Notes to
   Reviewer**. Shard is free, so leave "requires payment, non-free services or software"
   unticked. Add both screenshots, now or afterwards on the listing's edit page.
4. Submit. When the review passes, the listing is at
   `https://addons.mozilla.org/firefox/addon/<slug>`.

### 3. The slug and the credentials

The slug is the part of the listing's address after `/addon/`. The API key is at
<https://addons.mozilla.org/developers/addon/api/key/>: generate new credentials, which gives a
**JWT issuer** and a **JWT secret**.

```sh
gh variable set FIREFOX_STORE_SLUG -R manyeya/shard --body <slug>
gh secret set AMO_JWT_ISSUER -R manyeya/shard
gh secret set AMO_JWT_SECRET -R manyeya/shard
```

From then on, each release runs `web-ext sign --channel listed`, which submits the new version
for review without waiting for it.

## After a store's listing is live

1. **Set its variable and secrets**, as above.
2. **Release.** The next release's builds carry the IDs, so its setup offers that store, and the
   workflow sends that release's extension there. To try a listing in setup before then, set the
   same variable at run time (`SHARD_CHROME_STORE_ID`, `SHARD_EDGE_STORE_ID` or
   `SHARD_FIREFOX_STORE_SLUG`) when you start a copy with `--setup`.
3. **Link it from the website.** The download section of the site lists the stores in `STORES`
   in `site/main.js`, each with `url: null` until it's listed (it shows "On its way"). Fill in
   each store's `url` with its listing's address:

   ```js
   const STORES = [
     { name: "Chrome Web Store", url: "https://chromewebstore.google.com/detail/<CHROME_STORE_ID>" },
     { name: "Edge Add-ons", url: "https://microsoftedge.microsoft.com/addons/detail/<EDGE_STORE_ID>" },
     { name: "Firefox Add-ons", url: "https://addons.mozilla.org/firefox/addon/<FIREFOX_STORE_SLUG>" },
   ];
   ```

   The site deploys when `main` changes.
4. **Update the README**, whose extension section says the listings are on their way.
