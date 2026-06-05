import configureOpenAPI from "./lib/configure-open-api";
import createApp from "./lib/create-app";
import index from "./routes/index.route";
import mcp from "./routes/mcp.route";
import spl from "./routes/spl/spl.index";
import swap from "./routes/swap.route";
import transaction from "./routes/transaction.route";

const app = createApp();

configureOpenAPI(app);

app.route("/", index);
app.route("/", spl);
app.route("/", swap);
app.route("/", transaction);
app.route("/", mcp);

export type AppType = typeof app;

export default app;
