# syntax=docker/dockerfile:1.10

FROM node:22-bookworm-slim AS base
RUN corepack enable
WORKDIR /workspace

FROM base AS dependencies
COPY pnpm-workspace.yaml package.json* pnpm-lock.yaml* ./
COPY apps/web/package.json apps/web/package.json
RUN pnpm install --no-frozen-lockfile

FROM base AS builder
COPY --from=dependencies /workspace/node_modules ./node_modules
COPY --from=dependencies /workspace/apps/web/node_modules ./apps/web/node_modules
COPY . .
ARG BETTER_AUTH_URL
ARG GOOGLE_CLIENT_ID
ENV BETTER_AUTH_URL=${BETTER_AUTH_URL} GOOGLE_CLIENT_ID=${GOOGLE_CLIENT_ID}
ENV NEXT_TELEMETRY_DISABLED=1
RUN --mount=type=secret,id=better_auth_secret,env=BETTER_AUTH_SECRET \
    --mount=type=secret,id=google_client_secret,env=GOOGLE_CLIENT_SECRET \
    pnpm --dir apps/web build

FROM node:22-bookworm-slim AS runner
WORKDIR /app
ENV NODE_ENV=production NEXT_TELEMETRY_DISABLED=1 PORT=3000 HOSTNAME=0.0.0.0
COPY --chown=node:node --from=builder /workspace/apps/web/.next/standalone ./
COPY --chown=node:node --from=builder /workspace/apps/web/.next/static ./apps/web/.next/static
COPY --chown=node:node --from=builder /workspace/apps/web/public ./apps/web/public
USER node
EXPOSE 3000
CMD ["node", "apps/web/server.js"]
