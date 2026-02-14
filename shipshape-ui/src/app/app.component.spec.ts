import { DOCUMENT } from '@angular/common';
import { NavigationEnd, Router } from '@angular/router';
import { TestBed } from '@angular/core/testing';
import { signal } from '@angular/core';
import { Subject } from 'rxjs';

import { AppComponent } from './app.component';
import { AuthService } from './services/auth.service';

describe('AppComponent', () => {
  it('focuses the main region on navigation', () => {
    const events = new Subject<unknown>();
    const authStub = {
      user: signal(null),
      isAuthenticated: signal(false),
      signOut: jasmine.createSpy('signOut')
    };
    TestBed.configureTestingModule({
      imports: [AppComponent],
      providers: [
        {
          provide: Router,
          useValue: { events }
        },
        {
          provide: DOCUMENT,
          useValue: document
        },
        {
          provide: AuthService,
          useValue: authStub
        }
      ]
    });

    TestBed.overrideComponent(AppComponent, {
      set: {
        template: '<main id="main-content" tabindex="-1"></main>'
      }
    });

    const fixture = TestBed.createComponent(AppComponent);
    fixture.detectChanges();

    events.next(new NavigationEnd(1, '/dashboard', '/dashboard'));
    fixture.detectChanges();

    const main = fixture.nativeElement.querySelector('#main-content') as HTMLElement;
    expect(document.activeElement).toBe(main);
    events.complete();
  });

  it('defines primary navigation items', () => {
    const events = new Subject<unknown>();
    const authStub = {
      user: signal(null),
      isAuthenticated: signal(false),
      signOut: jasmine.createSpy('signOut')
    };
    TestBed.configureTestingModule({
      imports: [AppComponent],
      providers: [
        {
          provide: Router,
          useValue: { events }
        },
        {
          provide: DOCUMENT,
          useValue: document
        },
        {
          provide: AuthService,
          useValue: authStub
        }
      ]
    });
    TestBed.overrideComponent(AppComponent, {
      set: {
        template: '<main id="main-content"></main>'
      }
    });

    const fixture = TestBed.createComponent(AppComponent);
    const component = fixture.componentInstance;
    expect(component.navItems.length).toBe(4);
    expect(component.navItems[0].path).toBe('/dashboard');
  });

  it('signs out and redirects to login', () => {
    const events = new Subject<unknown>();
    const authStub = {
      user: signal({ id: '1', login: 'octo', githubId: '99' }),
      isAuthenticated: signal(true),
      signOut: jasmine.createSpy('signOut')
    };
    const navigate = jasmine.createSpy('navigate');
    TestBed.configureTestingModule({
      imports: [AppComponent],
      providers: [
        {
          provide: Router,
          useValue: { events, navigate }
        },
        {
          provide: DOCUMENT,
          useValue: document
        },
        {
          provide: AuthService,
          useValue: authStub
        }
      ]
    });
    TestBed.overrideComponent(AppComponent, {
      set: {
        template: '<main id="main-content"></main>'
      }
    });

    const fixture = TestBed.createComponent(AppComponent);
    fixture.componentInstance.signOut();

    expect(authStub.signOut).toHaveBeenCalled();
    expect(navigate).toHaveBeenCalledWith(['/login']);
  });
});
