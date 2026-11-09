export function isRegistrationEmail(value: unknown): boolean {
  // Match the email rule used by the API.
  return (
    typeof value === "string" &&
    /^[\w%+.-]+@[\d.A-Za-z-]+\.[A-Za-z]{2,}$/.test(value)
  );
}

export function isRegistrationUsername(value: unknown): boolean {
  return (
    typeof value === "string" &&
    value.length >= 5 &&
    /^[\dA-Za-z]*$/.test(value)
  );
}

export function isRegistrationPassword(value: unknown): boolean {
  return typeof value === "string" && value.length >= 8;
}

interface TokenCreatePayload {
  name?: string;
  acls?: string[];
}

export function parseTokenCreatePayload(encoded: string): TokenCreatePayload {
  const payload: unknown = JSON.parse(atob(encoded));
  if (typeof payload !== "object" || payload === null) {
    throw new Error("Invalid token create payload.");
  }

  const name = "name" in payload ? payload.name : undefined;
  const acls = "acls" in payload ? payload.acls : undefined;
  if (name !== undefined && typeof name !== "string") {
    throw new Error("Invalid token create payload name.");
  }
  if (
    acls !== undefined &&
    (!Array.isArray(acls) ||
      !acls.every((acl: unknown): acl is string => typeof acl === "string"))
  ) {
    throw new Error("Invalid token create payload ACLs.");
  }

  return { name, acls };
}
