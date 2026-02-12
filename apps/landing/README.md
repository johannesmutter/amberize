# Amberize Landing Page

Static marketing / landing page for Amberize, built with **SvelteKit** and the static adapter.

## Development

```bash
npm install
npm run dev
```

The dev server runs at `http://localhost:5173`.

## Build

```bash
npm run build
npm run preview   # preview the production build locally
```

Output goes to `build/`.

## Deployment (Fly.io)

The site is deployed as a static site served by **nginx** inside a Docker container on [Fly.io](https://fly.io). Machines are configured to **auto-stop** after a period of inactivity and **auto-start** on the next incoming request, keeping costs near zero.

### Prerequisites

- [Fly CLI](https://fly.io/docs/flyctl/install/) installed and authenticated (`fly auth login`)

### First-time setup

```bash
# Create the app on Fly.io (run from apps/landing/)
fly apps create --name amberize

# Deploy (fra = Frankfurt)
fly deploy -r fra
```

### Subsequent deploys

```bash
fly deploy -a amberize
```

### Useful commands

```bash
# Check app status
fly status -a amberize

# View logs
fly logs -a amberize

# Open in browser
fly open -a amberize

# List recent releases
fly releases -a amberize

# Rollback to a previous release image
fly releases --image -a amberize
fly deploy -a amberize --image registry.fly.io/amberize@sha256:<hash>
```

## Project structure

```
apps/landing/
├── src/
│   ├── lib/          # Shared components, i18n, utilities
│   └── routes/       # SvelteKit pages (+page.svelte, etc.)
├── static/           # Static assets (images, fonts, favicons)
├── Dockerfile        # Multi-stage: Node build → nginx serve
├── fly.toml          # Fly.io configuration
├── nginx.conf        # nginx config for static file serving
├── svelte.config.js  # SvelteKit config (adapter-static)
└── package.json
```
