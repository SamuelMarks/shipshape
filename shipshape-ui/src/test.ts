import { NgModule, provideZoneChangeDetection } from "@angular/core";
import { getTestBed } from "@angular/core/testing";
import {
  BrowserDynamicTestingModule,
  platformBrowserDynamicTesting,
} from "@angular/platform-browser-dynamic/testing";

@NgModule({
  providers: [provideZoneChangeDetection()],
})
export class ZoneTestModule {}

getTestBed().initTestEnvironment(
  [BrowserDynamicTestingModule, ZoneTestModule],
  platformBrowserDynamicTesting(),
);
