import configureOpenAPI from "./lib/configure-open-api";
import createApp from "./lib/create-app";
import index from "./routes/index.route";
import mcp from "./routes/mcp.route";
import spl from "./routes/spl/spl.index";

const app = createApp();

configureOpenAPI(app);

app.route("/", index);
app.route("/", spl);
app.route("/", mcp);

export type AppType = typeof app;

export default app;
