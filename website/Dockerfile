# zeropty.pubdoc.dev: build the docs site from the repository's Markdown, serve it with nginx.
# Build with the repository root as the context (docker build -f website/Dockerfile .): the site reads
# README.md, CHANGELOG.md, CONTRIBUTING.md, docs/ and api/ from there.
FROM node:22.23.2-alpine AS build
WORKDIR /repo/website
RUN corepack enable
COPY website/package.json website/yarn.lock website/.yarnrc.yml ./
RUN yarn install --immutable
COPY README.md README.ko.md CHANGELOG.md CONTRIBUTING.md /repo/
COPY docs /repo/docs
COPY api /repo/api
COPY website/ ./
RUN yarn build

FROM nginx:1.30.6-alpine
COPY website/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /repo/website/src/.vitepress/dist /usr/share/nginx/html
EXPOSE 80
