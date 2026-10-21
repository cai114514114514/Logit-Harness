export function solve(data: any): any {
  const path = data.path.split("/").filter(Boolean);
  for (const route of data.routes) {
    const parts = route.split("/").filter(Boolean); if (parts.length !== path.length) continue;
    const params: Record<string,string> = {};
    parts.forEach((part: string,index: number) => {if (part.startsWith(":")) params[part.slice(1)] = path[index];});
    return {route,params};
  }
  return null;
}
