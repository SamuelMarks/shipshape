export interface AuthConfigResponse {
  clientId: string;
  authorizeUrl: string;
  scopes: string[];
  redirectUri: string;
}

export interface AuthUser {
  id: string;
  login: string;
  githubId: string;
}

export interface AuthGithubRequest {
  code: string;
  redirectUri?: string | null;
}

export interface AuthGithubResponse {
  token: string;
  user: AuthUser;
}
