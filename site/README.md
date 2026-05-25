# bashkit.sh

Homepage for [bashkit](https://github.com/everruns/bashkit). Astro static site,
hosted on Cloudflare Workers (Static Assets / Pages-style output) at
[https://bashkit.sh](https://bashkit.sh).

## Develop

```bash
pnpm install
pnpm run dev       # local dev server on :4321
```

## Build

```bash
pnpm run build     # emits ./dist
pnpm run preview   # serve dist/ via wrangler
```

## Deploy

Deployment is intended to run from CI against the Cloudflare account that owns
the `bashkit.sh` zone. Manual deploy:

```bash
pnpm run deploy    # astro build && wrangler deploy
```

Configure the worker/project name and route in `wrangler.toml` or the
Cloudflare dashboard before the first deploy.

## Structure

```
site/
├── astro.config.mjs       # static output + cloudflare adapter
├── wrangler.toml          # cloudflare worker/pages config
├── src/
│   ├── layouts/Base.astro # html shell + SEO meta
│   ├── components/        # Nav, Footer
│   ├── pages/index.astro  # homepage
│   └── styles/global.css  # design tokens
└── public/                # favicon, robots.txt
```
