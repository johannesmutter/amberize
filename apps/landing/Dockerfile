# syntax = docker/dockerfile:1

# ---------------------------------------------------------------------------
# Build stage – install deps & produce static output
# ---------------------------------------------------------------------------
ARG NODE_VERSION=20.19.0
FROM node:${NODE_VERSION}-slim AS build

WORKDIR /app

# Install dependencies first (layer cache)
COPY --link .npmrc package.json package-lock.json* ./
RUN npm ci

# Copy source and build the static site
COPY --link . .
RUN npm run build

# ---------------------------------------------------------------------------
# Serve stage – lightweight nginx to serve static files
# ---------------------------------------------------------------------------
FROM nginx:1-alpine AS runtime

# Remove default nginx site
RUN rm /etc/nginx/conf.d/default.conf

# Add our nginx configuration
COPY nginx.conf /etc/nginx/conf.d/default.conf

# Copy built static files from the build stage
COPY --from=build /app/build /usr/share/nginx/html

EXPOSE 8080

CMD ["nginx", "-g", "daemon off;"]
