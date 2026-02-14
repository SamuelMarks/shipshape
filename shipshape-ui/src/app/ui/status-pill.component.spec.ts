import { TestBed } from '@angular/core/testing';

import { StatusPillComponent } from './status-pill.component';

describe('StatusPillComponent', () => {
  it('renders the label and tone class', () => {
    TestBed.configureTestingModule({
      imports: [StatusPillComponent]
    });
    const fixture = TestBed.createComponent(StatusPillComponent);
    fixture.componentRef.setInput('label', 'Healthy');
    fixture.componentRef.setInput('tone', 'good');
    fixture.detectChanges();

    const pill: HTMLElement | null = fixture.nativeElement.querySelector('.status-pill');
    expect(pill?.textContent).toContain('Healthy');
    expect(pill?.classList.contains('is-good')).toBeTrue();
  });
});
