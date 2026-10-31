#!/bin/bash

# This file starts up the Drop server by running migrations and then starting the executable
echo "[Drop] performing migrations..."
# The Prisma CLI is installed in /app/node_modules; the image has no pnpm.
/app/node_modules/.bin/prisma migrate deploy

# Actually start the application
node /app/app/server/index.mjs
