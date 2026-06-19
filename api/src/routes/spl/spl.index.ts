import { OpenAPIHono } from "@hono/zod-openapi";

import type { AppBindings } from "../../env";
import { openApiDefaultHook } from "../../lib/create-app";
import {
  balanceHandler,
  challengeHandler,
  depositHandler,
  initializeMintHandler,
  loginHandler,
  mintInitializationHandler,
  privateBalanceHandler,
  stealthPoolHandler,
  stealthPoolStatusHandler,
  transferQueueEnsureCrankHandler,
  transferHandler,
  undelegateEphemeralAtaHandler,
  withdrawHandler,
} from "./spl.handlers";
import {
  balanceRoute,
  challengeRoute,
  depositRoute,
  initializeMintRoute,
  loginRoute,
  mintInitializationRoute,
  privateBalanceRoute,
  stealthPoolRoute,
  stealthPoolStatusRoute,
  transferQueueEnsureCrankRoute,
  transferRoute,
  undelegateEphemeralAtaRoute,
  withdrawRoute,
} from "./spl.routes";

const app = new OpenAPIHono<{ Bindings: AppBindings }>({
  defaultHook: openApiDefaultHook,
});

app.openapi(depositRoute, depositHandler);
app.openapi(withdrawRoute, withdrawHandler);
app.openapi(initializeMintRoute, initializeMintHandler);
app.openapi(transferRoute, transferHandler);
app.openapi(undelegateEphemeralAtaRoute, undelegateEphemeralAtaHandler);
app.openapi(stealthPoolRoute, stealthPoolHandler);
app.openapi(stealthPoolStatusRoute, stealthPoolStatusHandler);
app.openapi(balanceRoute, balanceHandler);
app.openapi(privateBalanceRoute, privateBalanceHandler);
app.openapi(mintInitializationRoute, mintInitializationHandler);
app.openapi(transferQueueEnsureCrankRoute, transferQueueEnsureCrankHandler);
app.openapi(challengeRoute, challengeHandler);
app.openapi(loginRoute, loginHandler);

export default app;
