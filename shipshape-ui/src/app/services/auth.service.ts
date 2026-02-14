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

export interface AuthStorage {
  getToken(): string | null;
  setToken(token: string): void;
  clearToken(): void;
}

export class LocalStorageAuthStorage implements AuthStorage {
  private readonly key = "shipshape.token";

  getToken(): string | null {
    if (typeof globalThis.localStorage === "undefined") {
      return null;
    }
    return globalThis.localStorage.getItem(this.key);
  }

  setToken(token: string): void {
    if (typeof globalThis.localStorage === "undefined") {
      return;
    }
    globalThis.localStorage.setItem(this.key, token);
  }

  clearToken(): void {
    if (typeof globalThis.localStorage === "undefined") {
      return;
    }
    globalThis.localStorage.removeItem(this.key);
  }
}

export interface WindowRef {
  location: Location;
}

export const BROWSER_WINDOW = new InjectionToken<WindowRef>("BROWSER_WINDOW", {
  providedIn: "root",
  factory: () => globalThis as WindowRef,
});

export const AUTH_STORAGE = new InjectionToken<AuthStorage>("AUTH_STORAGE", {
  providedIn: "root",
  factory: () => new LocalStorageAuthStorage(),
});

@Injectable({ providedIn: "root" })
export class AuthService {
  private readonly http = inject(HttpClient);
  private readonly baseUrl = inject(API_BASE_URL);
  private readonly storage = inject(AUTH_STORAGE);

  private readonly configSignal = signal<AuthConfigResponse | null>(null);
  private readonly tokenSignal = signal<string | null>(this.storage.getToken());
  private readonly userSignal = signal<AuthUser | null>(null);

  readonly config = this.configSignal.asReadonly();
  readonly user = this.userSignal.asReadonly();
  readonly token = this.tokenSignal.asReadonly();
  readonly hasToken = computed(() => this.tokenSignal() !== null);
  readonly isAuthenticated = computed(
    () => this.tokenSignal() !== null && this.userSignal() !== null,
  );

  async getLoginUrl(): Promise<string> {
    const config = this.configSignal() ?? (await this.loadConfig());
    const scope = encodeURIComponent(config.scopes.join(" "));
    const redirect = encodeURIComponent(config.redirectUri);
    const clientId = encodeURIComponent(config.clientId);
    return `${config.authorizeUrl}?client_id=${clientId}&redirect_uri=${redirect}&scope=${scope}`;
  }

  async exchangeCode(
    code: string,
    redirectUri?: string,
  ): Promise<AuthGithubResponse> {
    const payload: AuthGithubRequest = { code, redirectUri };
    const response = await firstValueFrom(
      this.http.post<AuthGithubResponse>(this.url("/auth/github"), payload),
    );
    this.tokenSignal.set(response.token);
    this.storage.setToken(response.token);
    this.userSignal.set(response.user);
    return response;
  }

  async refreshSession(): Promise<boolean> {
    const token = this.tokenSignal();
    if (!token) {
      return false;
    }
    try {
      const user = await firstValueFrom(
        this.http.get<AuthUser>(this.url("/auth/me")),
      );
      this.userSignal.set(user);
      return true;
    } catch {
      this.signOut();
      return false;
    }
  }

  async ensureSession(): Promise<boolean> {
    if (this.userSignal()) {
      return true;
    }
    return this.refreshSession();
  }

  signOut(): void {
    this.storage.clearToken();
    this.tokenSignal.set(null);
    this.userSignal.set(null);
  }

  private async loadConfig(): Promise<AuthConfigResponse> {
    const response = await firstValueFrom(
      this.http.get<AuthConfigResponse>(this.url("/auth/config")),
    );
    this.configSignal.set(response);
    return response;
  }

  private url(path: string): string {
    return `${this.baseUrl}${path}`;
  }
}
