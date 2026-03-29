import { OpenAPIHono } from "@hono/zod-openapi";

import type { AppBindings } from "../../env";
import {
  balanceHandler,
  depositHandler,
  initializeMintHandler,
  mintInitializationHandler,
  privateBalanceHandler,
  transferHandler,
  withdrawHandler,
} from "./spl.handlers";
import {
  balanceRoute,
  depositRoute,
  initializeMintRoute,
  mintInitializationRoute,
  privateBalanceRoute,
  transferRoute,
  withdrawRoute,
} from "./spl.routes";

const app = new OpenAPIHono<{ Bindings: AppBindings }>();

app.openapi(depositRoute, depositHandler);
app.openapi(withdrawRoute, withdrawHandler);
app.openapi(initializeMintRoute, initializeMintHandler);
app.openapi(transferRoute, transferHandler);
app.openapi(balanceRoute, balanceHandler);
app.openapi(privateBalanceRoute, privateBalanceHandler);
app.openapi(mintInitializationRoute, mintInitializationHandler);

export default app;
