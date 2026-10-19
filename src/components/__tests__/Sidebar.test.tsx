import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import Sidebar from '../Sidebar';
import { describe, it, expect, vi } from 'vitest';

describe('Sidebar Component', () => {
    const mockProps = {
        devices: ['device1', 'device2'],
        runningDevices: ['device1'],
        onRefresh: vi.fn(),
        onKillAdb: vi.fn(),
        selectedDevice: 'device1',
        onSelectDevice: vi.fn(),
        onPair: vi.fn(),
        onConnect: vi.fn(),
        onUsbToWireless: vi.fn(),
        onDiagnose: vi.fn(),
        historyDevices: [],
        onFilePush: vi.fn(),
        isRefreshing: false
    };

    it('renders device list correctly', () => {
        render(<Sidebar {...mockProps} />);
        expect(screen.getByText('device1')).toBeInTheDocument();
        expect(screen.getByText('device2')).toBeInTheDocument();
    });

    it('shows "Live" status for running devices', () => {
        render(<Sidebar {...mockProps} />);
        const liveIndicator = screen.getByText('Live');
        expect(liveIndicator).toBeInTheDocument();
        expect(liveIndicator).toHaveClass('text-emerald-500');
    });

    it('calls onSelectDevice when a device is clicked', () => {
        render(<Sidebar {...mockProps} />);
        fireEvent.click(screen.getByText('device2'));
        expect(mockProps.onSelectDevice).toHaveBeenCalledWith('device2');
    });

    it('calls onRefresh when refresh button is clicked', () => {
        render(<Sidebar {...mockProps} />);
        fireEvent.click(screen.getByText(/refresh/i));
        expect(mockProps.onRefresh).toHaveBeenCalled();
    });

    it('switches tabs correctly', () => {
        render(<Sidebar {...mockProps} />);

        // USB default
        expect(screen.getByText('USB Setup Tip')).toBeInTheDocument();

        // Switch to Wireless
        fireEvent.click(screen.getByText('Wireless'));
        expect(screen.getByText('Wireless Setup Tip')).toBeInTheDocument();
    });


    it('runs USB -> Wi-Fi only for an authorized USB-style device serial', async () => {
        const onUsbToWireless = vi.fn().mockResolvedValue({ success: true });
        render(<Sidebar {...mockProps} devices={[
            '32c35470', '192.168.2.226:5555',
            'adb-phone._adb-tls-connect._tcp', 'emulator-5554'
        ]} onUsbToWireless={onUsbToWireless} />);
        fireEvent.click(screen.getByText('Wireless'));
        fireEvent.click(screen.getByTitle('32c35470'));
        await waitFor(() => expect(onUsbToWireless).toHaveBeenCalledWith('32c35470'));
        expect(screen.queryByTitle('192.168.2.226:5555')).not.toBeInTheDocument();
    });

    it('disables USB-to-Wi-Fi while a device refresh is active', () => {
        render(<Sidebar {...mockProps} devices={['32c35470']} isRefreshing={true} />);
        fireEvent.click(screen.getByText('Wireless'));
        expect(screen.getByTitle('32c35470')).toBeDisabled();
    });

    it('offers a verified same-IP wireless port only after explicit user choice',()=>{
        const onSelectDevice=vi.fn();
        const common={...mockProps,onSelectDevice,selectedDevice:'192.168.2.226:5555'};
        const {rerender}=render(<Sidebar {...common}
            devices={['192.168.2.226:42137']} runningDevices={[]}/>);
        const notice=screen.getByRole('region',{name:'Wireless port may have changed'});
        expect(notice).toHaveTextContent('a matching IP does not prove device identity');
        expect(onSelectDevice).not.toHaveBeenCalled();
        fireEvent.click(screen.getByRole('button',{name:'Use connected wireless port'}));
        expect(onSelectDevice).toHaveBeenCalledExactlyOnceWith('192.168.2.226:42137');
        rerender(<Sidebar {...common}
            devices={['192.168.2.226:42137','192.168.2.226:43210']} runningDevices={[]}/>);
        expect(screen.queryByRole('region',{name:'Wireless port may have changed'}))
            .not.toBeInTheDocument();
        rerender(<Sidebar {...common}
            devices={['192.168.2.226:5555']} runningDevices={[]}/>);
        expect(screen.queryByRole('region',{name:'Wireless port may have changed'}))
            .not.toBeInTheDocument();
    });

    it('shows unauthorized and offline devices without claiming they are ready', () => {
        render(<Sidebar {...mockProps} devices={[]} pendingDevices={[
            { serial: '32c35470', state: 'unauthorized' },
            { serial: '192.168.2.226:5555', state: 'offline' }
        ]} />);
        const notice = screen.getByRole('region', { name: 'Devices need attention' });
        expect(notice).toHaveTextContent('32c35470');
        expect(notice).toHaveTextContent('Authorization required');
        expect(notice).toHaveTextContent('Offline');
    });

    it('diagnoses a refused wireless port without attempting connection', async () => {
        const onDiagnose = vi.fn().mockResolvedValue({
            code: 'refused', address: '192.168.2.226:5555', alternatives: []
        });
        const onConnect = vi.fn();
        render(<Sidebar {...mockProps} devices={[]} onDiagnose={onDiagnose} onConnect={onConnect} />);
        fireEvent.click(screen.getByText('Wireless'));
        fireEvent.change(screen.getByPlaceholderText('192.168.x.x:5555'), {
            target: { value: '192.168.2.226:5555' }
        });
        fireEvent.click(screen.getByText('Diagnose connection'));
        await waitFor(() => expect(onDiagnose).toHaveBeenCalledWith('192.168.2.226:5555'));
        // Invocation can finish before React commits the async diagnosis to
        // the status DOM. Wait for the actual user-visible result, not only
        // for the mock service call to begin.
        expect(await screen.findByRole('status')).toHaveTextContent('refused this TCP port');
        expect(onConnect).not.toHaveBeenCalled();
    });

    it('applies a newly discovered dynamic port from diagnostics', async () => {
        const onDiagnose = vi.fn().mockResolvedValue({
            code: 'port_changed', address: '192.168.2.226:5555',
            alternatives: ['192.168.2.226:45231']
        });
        render(<Sidebar {...mockProps} devices={[]} onDiagnose={onDiagnose} />);
        fireEvent.click(screen.getByText('Wireless'));
        fireEvent.change(screen.getByPlaceholderText('192.168.x.x:5555'), {
            target: { value: '192.168.2.226:5555' }
        });
        fireEvent.click(screen.getByText('Diagnose connection'));
        const suggestion = await screen.findByText(/Use detected port: 192.168.2.226:45231/);
        fireEvent.click(suggestion);
        expect(screen.getByPlaceholderText('192.168.x.x:5555')).toHaveValue('192.168.2.226:45231');
    });

    it('opens a pairing modal for a discovered device on the pairing screen, and submits the code', async () => {
        const onPair = vi.fn().mockResolvedValue({ success: true });
        const mdnsProps = {
            ...mockProps,
            onPair,
            mdnsDevices: [
                { name: 'DiscoveredPhone', service: '_adb-tls-pairing._tcp', address: '192.168.0.104:37521' }
            ]
        };
        render(<Sidebar {...mdnsProps} />);

        // Switch to Wireless tab to see it
        fireEvent.click(screen.getByText('Wireless'));
        fireEvent.click(screen.getByText(/DiscoveredPhone/));

        // A device on the pairing screen never gets a doomed direct connect --
        // just a code field. The manual pairing section below always renders
        // its own "Pairing Code" input too, so the modal's is the 2nd one.
        expect(mockProps.onConnect).not.toHaveBeenCalled();
        const [, modalCodeInput] = screen.getAllByPlaceholderText('Pairing Code');
        fireEvent.change(modalCodeInput, { target: { value: '123456' } });
        fireEvent.click(screen.getByText('OK'));

        await waitFor(() => expect(onPair).toHaveBeenCalledWith('192.168.0.104:37521', '123456'));
    });

    it('collapses connect + pairing broadcasts from the same device into one entry', () => {
        const mdnsProps = {
            ...mockProps,
            mdnsDevices: [
                { name: 'DiscoveredPhone', service: '_adb-tls-connect._tcp', address: '192.168.0.104:42441' },
                { name: 'DiscoveredPhone', service: '_adb-tls-pairing._tcp', address: '192.168.0.104:33427' }
            ]
        };
        render(<Sidebar {...mdnsProps} />);

        fireEvent.click(screen.getByText('Wireless'));
        // Same physical device broadcasting both services shows up once, not
        // twice -- and as the pairing entry, the one actually actionable now.
        expect(screen.getAllByText(/DiscoveredPhone/)).toHaveLength(1);
        expect(screen.getByText(/192.168.0.104:33427/)).toBeInTheDocument();
    });

    it('shows a "not ready" info modal for a discovered device not yet on the pairing screen', () => {
        const mdnsProps = {
            ...mockProps,
            mdnsDevices: [
                { name: 'DiscoveredPhone', service: '_adb-tls-connect._tcp', address: '192.168.0.104:44321' }
            ]
        };
        render(<Sidebar {...mdnsProps} />);

        fireEvent.click(screen.getByText('Wireless'));
        fireEvent.click(screen.getByText(/DiscoveredPhone/));

        // Only advertising "_adb-tls-connect" means pairing was never done and
        // the pairing port/code don't exist yet -- no modal code field (only
        // the manual pairing section's own, always-rendered input), and no
        // connect/pair attempt that could only fail.
        expect(screen.getAllByPlaceholderText('Pairing Code')).toHaveLength(1);
        expect(mockProps.onConnect).not.toHaveBeenCalled();
        expect(mockProps.onPair).not.toHaveBeenCalled();
    });

    it('shows a loading indicator and the device identity while waiting on the pairing screen', () => {
        const mdnsProps = {
            ...mockProps,
            mdnsDevices: [
                { name: 'adb-DiscoveredPhone', service: '_adb-tls-connect._tcp', address: '192.168.0.104:44321' }
            ]
        };
        render(<Sidebar {...mdnsProps} />);

        fireEvent.click(screen.getByText('Wireless'));
        fireEvent.click(screen.getByText(/DiscoveredPhone/));

        // Which device it's waiting on (shown once in the list behind the
        // modal, once in the modal itself), that it's actively checking (not
        // a dead end), and a way out that doesn't claim to "OK" something
        // that never happened.
        expect(screen.getAllByText(/192.168.0.104:44321/)).toHaveLength(2);
        expect(screen.getByText('Checking for the pairing screen...')).toBeInTheDocument();
        expect(screen.getByText('Cancel')).toBeInTheDocument();
    });

    it('auto-transitions the open modal to the code field once the phone opens the pairing screen', () => {
        const notReady = { name: 'adb-DiscoveredPhone', service: '_adb-tls-connect._tcp', address: '192.168.0.104:44321' };
        const ready = { name: 'adb-DiscoveredPhone', service: '_adb-tls-pairing._tcp', address: '192.168.0.104:55555' };
        const { rerender } = render(<Sidebar {...mockProps} mdnsDevices={[notReady]} />);

        fireEvent.click(screen.getByText('Wireless'));
        fireEvent.click(screen.getByText(/DiscoveredPhone/));
        expect(screen.getAllByPlaceholderText('Pairing Code')).toHaveLength(1);

        // Simulate the background poll picking up the pairing broadcast --
        // same modal stays open and swaps straight to the code field, no
        // closing and reopening needed.
        rerender(<Sidebar {...mockProps} mdnsDevices={[ready]} />);
        expect(screen.getAllByPlaceholderText('Pairing Code')).toHaveLength(2);
    });

    it('disables the Recent Devices connect button while a refresh is already in flight', () => {
        render(<Sidebar {...mockProps} historyDevices={['192.168.1.128:5555']} isRefreshing={true} />);

        fireEvent.click(screen.getByText('Wireless'));
        // A rapid second click before the first connect attempt settles fired
        // a duplicate `adb connect` -- disabling this button while one is
        // already in flight, like the other two connect-triggering buttons
        // already do, is what prevents that.
        const connectBtn = screen.getByText('192.168.1.128:5555').closest('button');
        expect(connectBtn).toBeDisabled();
    });

    it('shows a clean label for an mDNS wireless-debugging serial, but keeps the full serial for selection', () => {
        const mdnsSerial = 'adb-VWGE5XZHOBAIAAJN-thoiAU._adb-tls-connect._tcp';
        const onSelectDevice = vi.fn();
        render(<Sidebar {...mockProps} devices={[mdnsSerial]} runningDevices={[]} onSelectDevice={onSelectDevice} />);

        expect(screen.getByText('VWGE5XZHOBAIAAJN-thoiAU')).toBeInTheDocument();
        expect(screen.queryByText(mdnsSerial)).not.toBeInTheDocument();

        fireEvent.click(screen.getByText('VWGE5XZHOBAIAAJN-thoiAU'));
        expect(onSelectDevice).toHaveBeenCalledWith(mdnsSerial);
    });

    it('rejects malformed wireless endpoints without making any ADB calls', () => {
        const onConnect = vi.fn();
        const onDiagnose = vi.fn();
        render(<Sidebar {...mockProps} devices={[]} onConnect={onConnect} onDiagnose={onDiagnose}/>);
        fireEvent.click(screen.getByText('Wireless'));
        fireEvent.change(screen.getByPlaceholderText('192.168.x.x:5555'), {
            target: { value: '-s 192.168.1.5' }
        });
        fireEvent.click(screen.getByText('Connect'));
        expect(onConnect).not.toHaveBeenCalled();
        expect(onDiagnose).not.toHaveBeenCalled();
        expect(screen.getByRole('status')).toHaveTextContent('valid IPv4 address');
    });

    it('automatically presents a failed connection diagnosis without a redundant TCP probe', async () => {
        const onConnect = vi.fn().mockResolvedValue({
            success: false, message: 'ADB refused',
            diagnosis: { code: 'port_changed', address:'192.168.2.226:5555',
                alternatives: ['192.168.2.226:42345','192.168.2.227:42345'] }
        });
        const onDiagnose = vi.fn();
        render(<Sidebar {...mockProps} devices={[]} onConnect={onConnect} onDiagnose={onDiagnose}/>);
        fireEvent.click(screen.getByText('Wireless'));
        fireEvent.change(screen.getByPlaceholderText('192.168.x.x:5555'), {
            target: { value: '192.168.2.226:5555' }
        });
        fireEvent.click(screen.getByText('Connect'));
        const result = await screen.findByRole('status');
        await waitFor(() => expect(result).toHaveTextContent('saved port is stale'));
        expect(onConnect).toHaveBeenCalledExactlyOnceWith('192.168.2.226:5555');
        expect(onDiagnose).not.toHaveBeenCalled();
        expect(screen.queryByText(/192.168.2.227:42345/)).not.toBeInTheDocument();
        fireEvent.click(screen.getByText(/Use detected port: 192.168.2.226:42345/));
        expect(screen.getByPlaceholderText('192.168.x.x:5555')).toHaveValue('192.168.2.226:42345');
        expect(onConnect).toHaveBeenCalledTimes(1); // Clicking suggestion never auto-connects.
    });

    it('locks repeated connect clicks and clearly reports ADB connection success', async () => {
        let finish!: (result: {success:boolean,address:string}) => void;
        const onConnect = vi.fn().mockImplementation(() => new Promise(resolve => {finish=resolve;}));
        render(<Sidebar {...mockProps} devices={[]} onConnect={onConnect}/>);
        fireEvent.click(screen.getByText('Wireless'));
        fireEvent.change(screen.getByPlaceholderText('192.168.x.x:5555'),{
            target:{value:'192.168.2.226'}
        });
        fireEvent.click(screen.getByText('Connect'));
        expect(screen.getByRole('status')).toHaveTextContent('Connecting to selected device');
        fireEvent.click(screen.getByText('...'));
        expect(onConnect).toHaveBeenCalledTimes(1);
        expect(onConnect).toHaveBeenCalledWith('192.168.2.226:5555');
        finish({success:true,address:'192.168.2.226:46555'});
        await waitFor(()=>expect(screen.getByRole('status')).toHaveTextContent('ADB connection established'));
        expect(screen.getByRole('status')).toHaveTextContent('192.168.2.226:46555');
    });

    it('distinguishes pairing success from a ready/authorized device session', async () => {
        const onPair = vi.fn().mockResolvedValue({success:true});
        const onConnect = vi.fn();
        render(<Sidebar {...mockProps} devices={[]} onPair={onPair} onConnect={onConnect}/>);
        fireEvent.click(screen.getByText('Wireless'));
        fireEvent.change(screen.getByPlaceholderText('IP:Port'), {
            target:{value:'192.168.2.226:37413'}
        });
        fireEvent.change(screen.getByPlaceholderText('Pairing Code'), {
            target:{value:'123456'}
        });
        fireEvent.click(screen.getByText('Start Pairing'));
        await waitFor(()=>expect(onPair).toHaveBeenCalledWith('192.168.2.226:37413','123456'));
        await waitFor(()=>expect(screen.getByRole('status')).toHaveTextContent('Pairing succeeded'));
        expect(screen.getByRole('status')).toHaveTextContent('pairing is not the same as connecting');
        expect(onConnect).not.toHaveBeenCalled();
    });

    it('labels discovered pairing versus connection ports and remembered offline devices', () => {
        render(<Sidebar {...mockProps} devices={[]} pendingDevices={[{
            serial:'192.168.2.226:5555',state:'offline'
        }]} historyDevices={['192.168.2.226:5555']} mdnsDevices={[
            {name:'A',service:'_adb-tls-pairing._tcp',address:'192.168.2.226:41334'},
            {name:'B',service:'_adb-tls-connect._tcp',address:'192.168.2.227:43117'}
        ]}/>);
        fireEvent.click(screen.getByText('Wireless'));
        expect(screen.getByText('Pairing port')).toBeInTheDocument();
        expect(screen.getByText('Connection port')).toBeInTheDocument();
        expect(screen.getAllByText('Offline').length).toBeGreaterThan(0);
        expect(screen.getByText(/retry automatically with backoff/)).toBeInTheDocument();
    });
});
