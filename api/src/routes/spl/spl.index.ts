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
  transferHandler,
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
  transferRoute,
  withdrawRoute,
} from "./spl.routes";

const app = new OpenAPIHono<{ Bindings: AppBindings }>({
  defaultHook: openApiDefaultHook,
});

app.openapi(depositRoute, depositHandler);
app.openapi(withdrawRoute, withdrawHandler);
app.openapi(initializeMintRoute, initializeMintHandler);
app.openapi(transferRoute, transferHandler);
app.openapi(balanceRoute, balanceHandler);
app.openapi(privateBalanceRoute, privateBalanceHandler);
app.openapi(mintInitializationRoute, mintInitializationHandler);
app.openapi(challengeRoute, challengeHandler);
app.openapi(loginRoute, loginHandler);

export default app;
