FROM node:22.23.3-alpine AS build
RUN apk add --no-cache python3 ca-certificates
WORKDIR /build/apps/web
COPY apps/web/package*.json ./
RUN npm ci --include=dev
COPY apps/web ./
COPY shaders /build/shaders
COPY Cargo.lock /build/Cargo.lock
COPY shader-core /build/shader-core
COPY tools/compilers.py /build/tools/compilers.py
RUN npm run build && gzip -k -9 dist/compilers/core/*.wasm dist/compilers/slang/*.wasm
FROM nginx:1.31.7-alpine
COPY apps/web/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /build/apps/web/dist /usr/share/nginx/html
EXPOSE 8000
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s CMD wget -q --spider http://127.0.0.1:8000/shaders/manifest.json || exit 1
CMD ["nginx","-g","daemon off;"]
