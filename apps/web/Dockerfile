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
ENV NEXT_TELEMETRY_DISABLED=1
RUN pnpm --dir apps/web build

FROM node:22-bookworm-slim AS runner
WORKDIR /app
ENV NODE_ENV=production NEXT_TELEMETRY_DISABLED=1 PORT=3000 HOSTNAME=0.0.0.0
COPY --from=builder /workspace/apps/web/.next/standalone ./
COPY --from=builder /workspace/apps/web/.next/static ./apps/web/.next/static
USER node
EXPOSE 3000
CMD ["node", "apps/web/server.js"]
