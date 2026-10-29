/**
 * @file Authentication state, token storage, and GitHub OAuth exchange service.
 */

import {
  Injectable,
  InjectionToken,
  computed,
  inject,
  signal,
} from "@angular/core";
import { HttpClient } from "@angular/common/http";
import { firstValueFrom } from "rxjs";

import { API_BASE_URL } from "./api.service";
import {
  AuthConfigResponse,
  AuthGithubRequest,
  AuthGithubResponse,
  AuthUser,
} from "./auth.models";

/**
 * Storage abstraction for persisting the ShipShape session token.
 */
export interface AuthStorage {
  /** Read the stored session token, or `null` if absent. */
  getToken(): string | null;
  /** Persist `token` to storage. */
  setToken(token: string): void;
  /** Remove the stored session token. */
  clearToken(): void;
}

/**
 * Browser `localStorage`-backed implementation of {@link AuthStorage}.
 */
export class LocalStorageAuthStorage implements AuthStorage {
  /** Storage key used for the ShipShape bearer token. */
  private readonly key = "shipshape.token";

  /**
   * Read the session token from `globalThis.localStorage`.
   *
   * @returns Stored token string or `null`.
   */
  getToken(): string | null {
    if (typeof globalThis.localStorage === "undefined") {
      return null;
    }
    return globalThis.localStorage.getItem(this.key);
  }

  /**
   * Write `token` into `globalThis.localStorage`.
   *
   * @param token - Session bearer token to store.
   */
  setToken(token: string): void {
    if (typeof globalThis.localStorage === "undefined") {
      return;
    }
    globalThis.localStorage.setItem(this.key, token);
  }

  /**
   * Remove the session token from `globalThis.localStorage`.
   */
  clearToken(): void {
    if (typeof globalThis.localStorage === "undefined") {
      return;
    }
    globalThis.localStorage.removeItem(this.key);
  }
}

/**
 * Minimal browser window reference exposing `location` for OAuth redirects.
 */
export interface WindowRef {
  /** Browser location object. */
  location: Location;
}

/**
 * Injection token providing the browser {@link WindowRef}.
 */
export const BROWSER_WINDOW = new InjectionToken<WindowRef>("BROWSER_WINDOW", {
  providedIn: "root",
  factory: () => globalThis as WindowRef,
});

/**
 * Injection token providing the {@link AuthStorage} implementation.
 */
export const AUTH_STORAGE = new InjectionToken<AuthStorage>("AUTH_STORAGE", {
  providedIn: "root",
  factory: () => new LocalStorageAuthStorage(),
});

/**
 * Service managing GitHub OAuth login URL construction, code exchange,
 * session verification, and token storage.
 */
@Injectable({ providedIn: "root" })
export class AuthService {
  /** Angular HTTP client. */
  private readonly http = inject(HttpClient);
  /** Backend API base URL. */
  private readonly baseUrl = inject(API_BASE_URL);
  /** Token storage adapter. */
  private readonly storage = inject(AUTH_STORAGE);

  /** Cached OAuth configuration signal. */
  private readonly configSignal = signal<AuthConfigResponse | null>(null);
  /** Active bearer token signal initialized from storage. */
  private readonly tokenSignal = signal<string | null>(this.storage.getToken());
  /** Current authenticated user profile signal. */
  private readonly userSignal = signal<AuthUser | null>(null);

  /** Read-only signal of the loaded OAuth configuration. */
  readonly config = this.configSignal.asReadonly();
  /** Read-only signal of the authenticated user profile. */
  readonly user = this.userSignal.asReadonly();
  /** Read-only signal of the active bearer token. */
  readonly token = this.tokenSignal.asReadonly();
  /** Computed signal indicating whether a bearer token is present. */
  readonly hasToken = computed(() => this.tokenSignal() !== null);
  /** Computed signal indicating whether both token and user profile are loaded. */
  readonly isAuthenticated = computed(
    () => this.tokenSignal() !== null && this.userSignal() !== null,
  );

  /**
   * Build the full GitHub OAuth authorization redirect URL.
   *
   * @returns Promise resolving to the GitHub authorize URL string.
   */
  async getLoginUrl(): Promise<string> {
    const config = this.configSignal() ?? (await this.loadConfig());
    const scope = encodeURIComponent(config.scopes.join(" "));
    const redirect = encodeURIComponent(config.redirectUri);
    const clientId = encodeURIComponent(config.clientId);
    return `${config.authorizeUrl}?client_id=${clientId}&redirect_uri=${redirect}&scope=${scope}`;
  }

  /**
   * Exchange a GitHub OAuth `code` for a ShipShape session token and user profile.
   *
   * @param code - Authorization code from the OAuth callback query string.
   * @param redirectUri - Optional callback redirect URI override.
   * @returns Promise resolving to the authentication response.
   */
  async exchangeCode(
    code: string,
    redirectUri?: string,
  ): Promise<AuthGithubResponse> {
    const payload: AuthGithubRequest = { code, redirectUri };
    const response = await firstValueFrom(
      this.http.post<AuthGithubResponse>(this.url("/api/auth/github"), payload),
    );
    this.tokenSignal.set(response.token);
    this.storage.setToken(response.token);
    this.userSignal.set(response.user);
    return response;
  }

  /**
   * Verify the current stored token against `GET /api/auth/me` and load the user profile.
   *
   * @returns Promise resolving to `true` if the token is valid, or `false` after signing out.
   */
  async refreshSession(): Promise<boolean> {
    const token = this.tokenSignal();
    if (!token) {
      return false;
    }
    try {
      const user = await firstValueFrom(
        this.http.get<AuthUser>(this.url("/api/auth/me")),
      );
      this.userSignal.set(user);
      return true;
    } catch {
      this.signOut();
      return false;
    }
  }

  /**
   * Ensure an active session is loaded, refreshing from `/api/auth/me` if needed.
   *
   * @returns Promise resolving to `true` when authenticated.
   */
  async ensureSession(): Promise<boolean> {
    if (this.userSignal()) {
      return true;
    }
    return this.refreshSession();
  }

  /**
   * Clear the stored token and reset authentication signals.
   */
  signOut(): void {
    this.storage.clearToken();
    this.tokenSignal.set(null);
    this.userSignal.set(null);
  }

  /**
   * Fetch OAuth configuration from `GET /api/auth/config` and cache it.
   *
   * @returns Promise resolving to the OAuth configuration response.
   */
  private async loadConfig(): Promise<AuthConfigResponse> {
    const response = await firstValueFrom(
      this.http.get<AuthConfigResponse>(this.url("/api/auth/config")),
    );
    this.configSignal.set(response);
    return response;
  }

  /**
   * Resolve a relative API `path` against {@link baseUrl}.
   *
   * @param path - Relative API path starting with `'/'`.
   * @returns Full endpoint URL string.
   */
  private url(path: string): string {
    return `${this.baseUrl}${path}`;
  }
}
