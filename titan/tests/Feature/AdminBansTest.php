<?php
namespace Titan\Tests\Feature;

use Illuminate\Foundation\Testing\RefreshDatabase;
use Titan\Tests\TestCase;

class AdminBansTest extends TestCase {
    use RefreshDatabase;

    public function testPageRedirectsToLoginWhenNotLoggedIn()
    {
        $this->followRedirects = false;

        $this->get(route('admin.banuser.index'))
            ->assertRedirect('/login')
            ->assertLocation(route('login'));
    }
}
