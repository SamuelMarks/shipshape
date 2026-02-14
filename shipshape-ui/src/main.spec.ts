import { AppComponent } from './app/app.component';
import {
  appProviders,
  bootstrapApp,
  bootstrapIfReady,
  monacoConfig,
  shouldBootstrap
} from './main';

describe('main bootstrap', () => {
  it('bootstraps the application with providers', async () => {
    const bootstrap = jasmine
      .createSpy('bootstrap')
      .and.returnValue(Promise.resolve({} as never));

    await bootstrapApp(bootstrap);

    expect(bootstrap).toHaveBeenCalled();
    const [rootComponent, options] = bootstrap.calls.mostRecent().args;
    expect(rootComponent).toBe(AppComponent);
    expect(options?.providers).toBe(appProviders);
  });

  it('exposes the Monaco config', () => {
    expect(monacoConfig.baseUrl).toContain('monaco');
  });

  it('detects test environments and guards bootstrap', async () => {
    expect(shouldBootstrap({ __karma__: {} })).toBeFalse();
    expect(shouldBootstrap({ jasmine: {} })).toBeFalse();
    expect(shouldBootstrap({})).toBeTrue();

    const bootstrap = jasmine
      .createSpy('bootstrap')
      .and.returnValue(Promise.resolve({} as never));
    await bootstrapIfReady({}, bootstrap);
    expect(bootstrap).toHaveBeenCalled();
  });

  it('skips bootstrap when test env is detected', () => {
    const bootstrap = jasmine
      .createSpy('bootstrap')
      .and.returnValue(Promise.resolve({} as never));
    const result = bootstrapIfReady({ __karma__: {} }, bootstrap);
    expect(result).toBeNull();
    expect(bootstrap).not.toHaveBeenCalled();

    expect(bootstrapIfReady()).toBeNull();
  });

  it('routes bootstrap errors to the handler', async () => {
    const bootstrap = jasmine
      .createSpy('bootstrap')
      .and.returnValue(Promise.reject(new Error('boom')));
    const onError = jasmine.createSpy('onError');
    await bootstrapIfReady({}, bootstrap, onError);
    expect(onError).toHaveBeenCalled();
  });
});
