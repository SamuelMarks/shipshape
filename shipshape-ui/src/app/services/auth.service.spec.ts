import { TestBed } from "@angular/core/testing";
import {
  HttpTestingController,
  provideHttpClientTesting,
} from "@angular/common/http/testing";
import { provideHttpClient } from "@angular/common/http";

import {
  AuthService,
  AUTH_STORAGE,
  AuthStorage,
  BROWSER_WINDOW,
  LocalStorageAuthStorage,
} from "./auth.service";
import { API_BASE_URL } from "./api.service";

class MemoryStorage implements AuthStorage {
  constructor(private token: string | null = null) {}

  getToken(): string | null {
    return this.token;
  }

  setToken(token: string): void {
    this.token = token;
  }

  clearToken(): void {
    this.token = null;
  }
}

function setup(initialToken: string | null = null) {
  const storage = new MemoryStorage(initialToken);
  TestBed.configureTestingModule({
    providers: [
      provideHttpClient(),
      provideHttpClientTesting(),
      { provide: API_BASE_URL, useValue: "http://api" },
      { provide: AUTH_STORAGE, useValue: storage },
    ],
  });
  return {
    service: TestBed.inject(AuthService),
    http: TestBed.inject(HttpTestingController),
    storage,
  };
}

describe("AuthService", () => {
  it("builds the GitHub authorize URL", async () => {
    const { service, http } = setup();
    const promise = service.getLoginUrl();

    const req = http.expectOne("http://api/auth/config");
    req.flush({
      clientId: "client",
      authorizeUrl: "https://github.com/login/oauth/authorize",
      scopes: ["read:user"],
      redirectUri: "http://localhost/auth/callback",
    });

    const url = await promise;
    expect(url).toContain("client_id=client");
    expect(url).toContain("scope=read%3Auser");
  });

  it("stores tokens on exchange", async () => {
    const { service, http, storage } = setup();
    const promise = service.exchangeCode("code-1");

    const req = http.expectOne("http://api/auth/github");
    expect(req.request.body.code).toBe("code-1");
    req.flush({
      token: "shipshape-token",
      user: { id: "1", login: "octo", githubId: "99" },
    });

    const response = await promise;
    expect(response.token).toBe("shipshape-token");
    expect(storage.getToken()).toBe("shipshape-token");
    expect(service.user()?.login).toBe("octo");
  });

  it("refreshes the session when a token exists", async () => {
    const { service, http } = setup("shipshape-token");
    const promise = service.refreshSession();

    const req = http.expectOne("http://api/auth/me");
    req.flush({ id: "1", login: "octo", githubId: "99" });

    const result = await promise;
    expect(result).toBeTrue();
    expect(service.user()?.login).toBe("octo");
  });

  it("returns false when no token is present", async () => {
    const { service, http } = setup();
    const result = await service.refreshSession();

    http.expectNone("http://api/auth/me");
    expect(result).toBeFalse();
  });

  it("clears the session on refresh failure", async () => {
    const { service, http, storage } = setup("shipshape-token");
    const promise = service.refreshSession();

    const req = http.expectOne("http://api/auth/me");
    req.flush("Unauthorized", { status: 401, statusText: "Unauthorized" });

    const result = await promise;
    expect(result).toBeFalse();
    expect(storage.getToken()).toBeNull();
  });

  it("short-circuits ensureSession when a user is set", async () => {
    const { service, http } = setup();
    const exchange = service.exchangeCode("code-2");
    const exchangeReq = http.expectOne("http://api/auth/github");
    exchangeReq.flush({
      token: "shipshape-token",
      user: { id: "1", login: "octo", githubId: "99" },
    });
    await exchange;

    const result = await service.ensureSession();

    http.expectNone("http://api/auth/me");
    expect(result).toBeTrue();
  });

  it("refreshes the session when user data is missing", async () => {
    const { service, http } = setup("shipshape-token");
    const promise = service.ensureSession();

    const req = http.expectOne("http://api/auth/me");
    req.flush({ id: "1", login: "octo", githubId: "99" });

    const result = await promise;
    expect(result).toBeTrue();
  });

  it("signs out and clears storage", async () => {
    const { service, http, storage } = setup();
    const exchange = service.exchangeCode("code-3");
    const exchangeReq = http.expectOne("http://api/auth/github");
    exchangeReq.flush({
      token: "shipshape-token",
      user: { id: "1", login: "octo", githubId: "99" },
    });
    await exchange;

    service.signOut();

    expect(storage.getToken()).toBeNull();
    expect(service.user()).toBeNull();
  });

  it("exposes hasToken and isAuthenticated signals", async () => {
    const { service, http } = setup();
    expect(service.hasToken()).toBeFalse();
    expect(service.isAuthenticated()).toBeFalse();

    const exchange = service.exchangeCode("code-4");
    const exchangeReq = http.expectOne("http://api/auth/github");
    exchangeReq.flush({
      token: "shipshape-token",
      user: { id: "1", login: "octo", githubId: "99" },
    });
    await exchange;

    expect(service.hasToken()).toBeTrue();
    expect(service.isAuthenticated()).toBeTrue();
  });
});

describe("AuthService tokens", () => {
  it("uses the default window factory", () => {
    TestBed.configureTestingModule({});
    const windowRef = TestBed.inject(BROWSER_WINDOW);
    expect(windowRef).toBe(globalThis as unknown as Window);
  });

  it("uses the default storage factory", () => {
    TestBed.configureTestingModule({});
    const storage = TestBed.inject(AUTH_STORAGE);
    expect(storage).toEqual(jasmine.any(LocalStorageAuthStorage));
  });

  it("reads and writes via localStorage when available", () => {
    const storage = new LocalStorageAuthStorage();
    storage.setToken("shipshape-token");
    expect(storage.getToken()).toBe("shipshape-token");
    storage.clearToken();
    expect(storage.getToken()).toBeNull();
  });

  it("handles missing localStorage gracefully", () => {
    const original = Object.getOwnPropertyDescriptor(
      globalThis,
      "localStorage",
    );
    Object.defineProperty(globalThis, "localStorage", {
      value: undefined,
      configurable: true,
    });

    const storage = new LocalStorageAuthStorage();
    expect(storage.getToken()).toBeNull();
    storage.setToken("token");
    storage.clearToken();

    if (original) {
      Object.defineProperty(globalThis, "localStorage", original);
    }
  });
});
