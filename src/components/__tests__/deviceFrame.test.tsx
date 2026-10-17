import { render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import SessionBehavior from '../SessionBehavior';

afterEach(() => vi.restoreAllMocks());

describe('single-source native phone bezel setting', () => {
  it('does not duplicate the home phone frame control in behavior settings on Windows', () => {
    vi.spyOn(window.navigator, 'userAgent', 'get')
      .mockReturnValue('Mozilla/5.0 (Windows NT 10.0; Win64; x64)');

    render(<SessionBehavior
      config={{ device: 'phone', sessionMode: 'mirror', deviceFrame: true }}
      setConfig={vi.fn()}
    />);
    expect(screen.queryByText('iPhone-style slim titanium bezel')).not.toBeInTheDocument();
    expect(screen.queryByRole('switch',{name:'Borderless'})).not.toBeInTheDocument();
    expect(screen.queryByRole('switch',{name:/Remember window position/})).not.toBeInTheDocument();
  });

  it('hides Windows-only device frame option on other operating systems', () => {
    vi.spyOn(window.navigator, 'userAgent', 'get').mockReturnValue('Linux x86_64');
    render(<SessionBehavior
      config={{ device: 'phone', sessionMode: 'mirror', deviceFrame: true }}
      setConfig={vi.fn()}
    />);
    expect(screen.queryByText('iPhone-style slim titanium bezel')).not.toBeInTheDocument();
  });
});
