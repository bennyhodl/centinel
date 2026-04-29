# Centinel web container.
#
# Runs the Next.js 16 app + the Python dispatcher (`bin/centinel`).
# Hermes itself is NOT in the image — it's mounted from the host (see
# docker-compose.yml).
#
# Build context = repo root, so we can COPY app/ and have access to the
# dispatcher path layout when invoked from inside the container.

# ──── deps (pnpm install with native compile toolchain for better-sqlite3) ────
FROM node:20-alpine AS deps
RUN apk add --no-cache python3 py3-pip make g++ \
    && corepack enable
WORKDIR /app/app
# Copy only what's needed to install deps so this layer caches well.
COPY app/package.json app/pnpm-lock.yaml app/pnpm-workspace.yaml ./
RUN --mount=type=cache,id=pnpm,target=/root/.local/share/pnpm/store \
    pnpm install --frozen-lockfile

# ──── builder (next build) ────────────────────────────────────────────────────
FROM node:20-alpine AS builder
RUN apk add --no-cache python3 make g++ \
    && corepack enable
WORKDIR /app/app
COPY --from=deps /app/app/node_modules ./node_modules
COPY app/ ./
ENV NEXT_TELEMETRY_DISABLED=1
RUN pnpm build

# ──── runner (production server + python3 dispatcher) ─────────────────────────
FROM node:20-alpine AS runner
RUN apk add --no-cache python3 py3-yaml \
    && corepack enable
WORKDIR /app/app
ENV NODE_ENV=production
ENV NEXT_TELEMETRY_DISABLED=1
ENV PORT=3000
# Pull in the built app + production node_modules.
COPY --from=builder /app/app/.next ./.next
COPY --from=builder /app/app/public ./public
COPY --from=builder /app/app/package.json ./package.json
COPY --from=builder /app/app/next.config.ts ./next.config.ts
COPY --from=deps /app/app/node_modules ./node_modules
EXPOSE 3000
# pnpm start invokes `next start` per app/package.json.
CMD ["pnpm", "start"]
