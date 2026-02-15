import { TestBed } from "@angular/core/testing";
import {
  HttpClient,
  provideHttpClient,
  withInterceptors,
} from "@angular/common/http";
import {
  HttpTestingController,
  provideHttpClientTesting,
} from "@angular/common/http/testing";
import { signal } from "@angular/core";

import { authInterceptor } from "./auth.interceptor";
import { AuthService } from "./auth.service";

describe("authInterceptor", () => {
  it("adds the authorization header for protected routes", () => {
    const authStub = {
      token: signal("shipshape-token"),
    };

    TestBed.configureTestingModule({
      providers: [
        provideHttpClient(withInterceptors([authInterceptor])),
        provideHttpClientTesting(),
        { provide: AuthService, useValue: authStub },
      ],
    });

    const http = TestBed.inject(HttpClient);
    const controller = TestBed.inject(HttpTestingController);

    http.get("/api/dashboard").subscribe();
    const req = controller.expectOne("/dashboard");

    expect(req.request.headers.get("Authorization")).toBe(
      "Bearer shipshape-token",
    );
    req.flush({});
  });

  it("skips auth for OAuth endpoints", () => {
    const authStub = {
      token: signal("shipshape-token"),
    };

    TestBed.configureTestingModule({
      providers: [
        provideHttpClient(withInterceptors([authInterceptor])),
        provideHttpClientTesting(),
        { provide: AuthService, useValue: authStub },
      ],
    });

    const http = TestBed.inject(HttpClient);
    const controller = TestBed.inject(HttpTestingController);

    http.get("/api/auth/config").subscribe();
    const req = controller.expectOne("/auth/config");

    expect(req.request.headers.has("Authorization")).toBeFalse();
    req.flush({});
  });

  it("skips auth when no token is available", () => {
    const authStub = {
      token: signal(null),
    };

    TestBed.configureTestingModule({
      providers: [
        provideHttpClient(withInterceptors([authInterceptor])),
        provideHttpClientTesting(),
        { provide: AuthService, useValue: authStub },
      ],
    });

    const http = TestBed.inject(HttpClient);
    const controller = TestBed.inject(HttpTestingController);

    http.get("/api/dashboard").subscribe();
    const req = controller.expectOne("/dashboard");

    expect(req.request.headers.has("Authorization")).toBeFalse();
    req.flush({});
  });
});
