# Build context is the REPOSITORY ROOT, not apps/web. The shader corpus at
# shaders/ sits above the web app and must be copied into the build.
FROM node:22-alpine AS build

WORKDIR /build

COPY apps/web/package.json apps/web/package-lock.json ./apps/web/
RUN cd apps/web && npm ci

COPY apps/web ./apps/web
COPY shaders ./shaders

# Triggers the prebuild step, which emits public/shaders/ from ../../../shaders.
RUN cd apps/web && npm run build

FROM nginx:alpine

COPY apps/web/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /build/apps/web/dist /usr/share/nginx/html

EXPOSE 8000
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s \
  CMD wget -q --spider http://127.0.0.1:8000/shaders/manifest.json || exit 1

CMD ["nginx", "-g", "daemon off;"]
