import { render, screen, fireEvent } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import SessionBehavior from '../SessionBehavior';

afterEach(() => vi.restoreAllMocks());

describe('iPhone-style live device shell setting', () => {
  it('switches device frame on and off independently of video controls on Windows', () => {
    vi.spyOn(window.navigator, 'userAgent', 'get')
      .mockReturnValue('Mozilla/5.0 (Windows NT 10.0; Win64; x64)');

    const setConfig = vi.fn();
    render(<SessionBehavior
      config={{ device: 'phone', sessionMode: 'mirror', deviceFrame: true }}
      setConfig={setConfig}
    />);
    const label = screen.getByText('iPhone-style device frame');
    fireEvent.click(label);
    expect(setConfig).toHaveBeenCalledWith(
      expect.objectContaining({ deviceFrame: false, device: 'phone', sessionMode: 'mirror' })
    );
  });

  it('hides Windows-only device frame option on other operating systems', () => {
    vi.spyOn(window.navigator, 'userAgent', 'get').mockReturnValue('Linux x86_64');
    render(<SessionBehavior
      config={{ device: 'phone', sessionMode: 'mirror', deviceFrame: true }}
      setConfig={vi.fn()}
    />);
    expect(screen.queryByText('iPhone-style device frame')).not.toBeInTheDocument();
  });
});
