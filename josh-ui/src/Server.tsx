export function getBase(): string {
    const baseUrl = import.meta.env.BASE_URL;
    const trimmed = baseUrl.replace(/\/$/, '');

    return trimmed === '' ? '/' : trimmed;
}

export function getServer(): string {
    return window.location.origin
}
