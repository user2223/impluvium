export function launch(name: string) {
  const disableEnv = `EXAMPLE_SERVICE_${name.toUpperCase()}`;
  if (!process.env[disableEnv]) {
    start(name);
  }
}
