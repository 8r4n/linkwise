import { publishedRelease } from "../releases.mjs";

export async function GET() {
  return Response.json(await publishedRelease());
}
