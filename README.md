# Install serverless-rust plugin
```
npx sls plugin install -n serverless-rust
```

# Running in dev mode
```
npx sls invoke local -f tuser -d "$(cat tests/payload.json)"
```

# Deploy
```
npx sls deploy
```

