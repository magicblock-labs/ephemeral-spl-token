import { OpenAPIHono } from "@hono/zod-openapi";

import type { AppBindings } from "../../env";
import {
  balanceHandler,
  depositHandler,
  privateBalanceHandler,
  transferHandler,
  withdrawHandler,
} from "./spl.handlers";
import {
  balanceRoute,
  depositRoute,
  privateBalanceRoute,
  transferRoute,
  withdrawRoute,
} from "./spl.routes";

const app = new OpenAPIHono<{ Bindings: AppBindings }>();

app.openapi(depositRoute, depositHandler);
app.openapi(withdrawRoute, withdrawHandler);
app.openapi(transferRoute, transferHandler);
app.openapi(balanceRoute, balanceHandler);
app.openapi(privateBalanceRoute, privateBalanceHandler);

export default app;
