import { TestBed } from "@angular/core/testing";

import { LoginPage } from "./login.page";
import { AuthService, BROWSER_WINDOW } from "../../services/auth.service";

class MockLocation {
  assign = jasmine.createSpy("assign");
}

describe("LoginPage", () => {
  it("redirects to the GitHub authorize URL", async () => {
    const location = new MockLocation();
    TestBed.configureTestingModule({
      imports: [LoginPage],
      providers: [
        {
          provide: AuthService,
          useValue: {
            getLoginUrl: () =>
              Promise.resolve("https://github.com/login/oauth"),
          },
        },
        {
          provide: BROWSER_WINDOW,
          useValue: { location },
        },
      ],
    });

    const fixture = TestBed.createComponent(LoginPage);
    await fixture.componentInstance.startLogin();

    expect(location.assign).toHaveBeenCalledWith(
      "https://github.com/login/oauth",
    );
  });
});
