# josh-github

> A GitHub App built with [Probot](https://github.com/probot/probot) that manages change-based workflows for josh on github

## Setup

```sh
# Install dependencies
npm install

# Build the ts app
npm run build

# Run the bot
LOG_LEVEL=trace npm start
```

## Docker

```sh
# 1. Build container
docker build -t josh-github .

# 2. Start container
docker run -e APP_ID=<app-id> -e PRIVATE_KEY=<pem-value> josh-github
```
