import { TestBed } from "@angular/core/testing";
import { BehaviorSubject, Subject } from "rxjs";
import { ActivatedRoute, Router, convertToParamMap } from "@angular/router";

import { AuthCallbackPage } from "./auth-callback.page";
import { AuthService } from "../../services/auth.service";

function setupRoute(params: Record<string, string>) {
  const subject = new BehaviorSubject(convertToParamMap(params));
  return {
    queryParamMap: subject.asObservable(),
    push: (next: Record<string, string>) =>
      subject.next(convertToParamMap(next)),
  };
}

describe("AuthCallbackPage", () => {
  it("handles a missing OAuth code", async () => {
    const routeStub = setupRoute({});
    const authStub = {
      exchangeCode: jasmine.createSpy("exchangeCode"),
    };
    const events = new Subject<unknown>();
    const urlTree = {} as unknown;
    TestBed.configureTestingModule({
      imports: [AuthCallbackPage],
      providers: [
        { provide: ActivatedRoute, useValue: routeStub },
        { provide: AuthService, useValue: authStub },
        {
          provide: Router,
          useValue: {
            navigate: jasmine.createSpy("navigate"),
            createUrlTree: jasmine
              .createSpy("createUrlTree")
              .and.returnValue(urlTree),
            serializeUrl: jasmine
              .createSpy("serializeUrl")
              .and.returnValue("/login"),
            events,
          },
        },
      ],
    });

    const fixture = TestBed.createComponent(AuthCallbackPage);
    await fixture.whenStable();

    expect(fixture.componentInstance.status()).toBe("error");
    expect(authStub.exchangeCode).not.toHaveBeenCalled();
  });

  it("exchanges the OAuth code and redirects", async () => {
    const routeStub = setupRoute({ code: "abc" });
    const authStub = {
      exchangeCode: jasmine.createSpy("exchangeCode").and.resolveTo({
        token: "token",
        user: { id: "1", login: "octo", githubId: "99" },
      }),
    };
    const events = new Subject<unknown>();
    const urlTree = {} as unknown;
    const routerStub = {
      navigate: jasmine.createSpy("navigate").and.resolveTo(true),
      createUrlTree: jasmine
        .createSpy("createUrlTree")
        .and.returnValue(urlTree),
      serializeUrl: jasmine.createSpy("serializeUrl").and.returnValue("/login"),
      events,
    };

    TestBed.configureTestingModule({
      imports: [AuthCallbackPage],
      providers: [
        { provide: ActivatedRoute, useValue: routeStub },
        { provide: AuthService, useValue: authStub },
        { provide: Router, useValue: routerStub },
      ],
    });

    const fixture = TestBed.createComponent(AuthCallbackPage);
    await fixture.whenStable();

    expect(authStub.exchangeCode).toHaveBeenCalledWith("abc");
    expect(routerStub.navigate).toHaveBeenCalledWith(["/dashboard"]);
    expect(fixture.componentInstance.status()).toBe("success");
  });

  it("reports an exchange error", async () => {
    const routeStub = setupRoute({ code: "err" });
    const authStub = {
      exchangeCode: jasmine
        .createSpy("exchangeCode")
        .and.rejectWith(new Error("boom")),
    };
    const events = new Subject<unknown>();
    const urlTree = {} as unknown;
    TestBed.configureTestingModule({
      imports: [AuthCallbackPage],
      providers: [
        { provide: ActivatedRoute, useValue: routeStub },
        { provide: AuthService, useValue: authStub },
        {
          provide: Router,
          useValue: {
            navigate: jasmine.createSpy("navigate"),
            createUrlTree: jasmine
              .createSpy("createUrlTree")
              .and.returnValue(urlTree),
            serializeUrl: jasmine
              .createSpy("serializeUrl")
              .and.returnValue("/login"),
            events,
          },
        },
      ],
    });

    const fixture = TestBed.createComponent(AuthCallbackPage);
    await fixture.whenStable();

    expect(fixture.componentInstance.status()).toBe("error");
  });
});
