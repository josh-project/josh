FROM node:12-slim
WORKDIR /usr/src/app
COPY package.json package-lock.json ./
RUN npm ci --production && npm cache clean --force
ENV NODE_ENV="production"
COPY . .
CMD [ "npm", "start" ]
