/**
 * @file Data transfer object (DTO) interfaces for ShipShape OAuth authentication.
 */

/** OAuth configuration returned by `GET /api/auth/config`. */
export interface AuthConfigResponse {
  /** GitHub OAuth application client ID. */
  clientId: string;
  /** GitHub OAuth authorization endpoint URL. */
  authorizeUrl: string;
  /** Requested OAuth scopes. */
  scopes: string[];
  /** UI callback redirect URI. */
  redirectUri: string;
}

/** Authenticated user profile returned by `/api/auth/github` and `/api/auth/me`. */
export interface AuthUser {
  /** Internal ShipShape user UUID. */
  id: string;
  /** GitHub login handle. */
  login: string;
  /** GitHub numeric user ID string. */
  githubId: string;
}

/** Request payload for `POST /api/auth/github`. */
export interface AuthGithubRequest {
  /** OAuth authorization code received from GitHub callback. */
  code: string;
  /** Optional redirect URI override. */
  redirectUri?: string | null;
}

/** Response payload from `POST /api/auth/github`. */
export interface AuthGithubResponse {
  /** Issued ShipShape bearer session token. */
  token: string;
  /** Authenticated user profile. */
  user: AuthUser;
}
