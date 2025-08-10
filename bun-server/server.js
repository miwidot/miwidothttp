import { serve, file } from "bun";
import { join } from "path";

serve({
  port: 3000,
  async fetch(req) {
    const url = new URL(req.url);
    const filePath = join("./static", url.pathname === "/" ? "/index.html" : url.pathname);
    
    try {
      return new Response(file(filePath));
    } catch (err) {
      return new Response("Not Found", { status: 404 });
    }
  },
});